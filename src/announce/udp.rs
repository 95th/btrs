use crate::announce::{AnnounceRequest, AnnounceResponse};
use crate::peer::Peer;
use byteorder::{ReadBytesExt, WriteBytesExt, BE};
use log::trace;
use rand::thread_rng;
use rand::Rng;
use std::io::Cursor;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tokio::net::{lookup_host, UdpSocket};
use url::Url;

const TRACKER_CONSTANT: u64 = 0x0417_2710_1980;

mod action {
    pub const CONNECT: u32 = 0;
    pub const ANNOUNCE: u32 = 1;
}

pub async fn announce(req: AnnounceRequest, buf: &mut [u8]) -> crate::Result<AnnounceResponse> {
    let mut t = UdpTracker::new(req).await?;
    t.connect(buf).await?;
    t.announce(buf).await
}

struct UdpTracker {
    socket: UdpSocket,
    addr: SocketAddr,
    req: AnnounceRequest,
    conn_id: u64,
    txn_id: u32,
    pending_action: u32,
}

impl UdpTracker {
    pub async fn new(req: AnnounceRequest) -> crate::Result<UdpTracker> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
        let addr = match req.resolved_addr {
            Some(a) => a,
            None => resolve_addr(&req.url).await?,
        };

        Ok(UdpTracker {
            socket,
            addr,
            req,
            conn_id: 0,
            txn_id: 0,
            pending_action: 0,
        })
    }

    fn update_txn_id(&mut self) {
        self.txn_id = thread_rng().gen();
    }

    async fn connect(&mut self, buf: &mut [u8]) -> crate::Result<()> {
        self.update_txn_id();

        trace!("Sending connect to {}, txn id: {}", self.addr, self.txn_id);

        let n = self.write_connect(buf)?;
        let written = self.socket.send_to(&buf[..n], &self.addr).await?;
        if written != n {
            return Err("Error sending data".into());
        }

        self.pending_action = action::CONNECT;

        let (_, mut c) = self.read_response(buf, 16).await?;
        let conn_id = c.read_u64::<BE>()?;
        trace!("conn_id: {}", conn_id);
        self.conn_id = conn_id;

        Ok(())
    }

    async fn announce(&mut self, buf: &mut [u8]) -> crate::Result<AnnounceResponse> {
        self.update_txn_id();

        trace!("Sending announce to {}, txn id: {}", self.addr, self.txn_id);

        let n = self.write_announce(buf)?;
        let written = self.socket.send_to(&buf[..n], &self.addr).await?;
        if written != n {
            return Err("Error sending data".into());
        }

        self.pending_action = action::ANNOUNCE;

        let (len, mut c) = self.read_response(buf, 20).await?;

        let interval = c.read_u32::<BE>()?;
        let leechers = c.read_u32::<BE>()?;
        let seeders = c.read_u32::<BE>()?;

        trace!("interval: {}", interval);
        trace!("seeders: {}", seeders);
        trace!("leechers: {}", leechers);

        let mut n = len - 20;
        if n % 6 != 0 {
            return Err("IPs should be 6 byte each".into());
        }

        let mut peers = hashset![];
        while n > 0 {
            let ip_addr = c.read_u32::<BE>()?;
            let port = c.read_u16::<BE>()?;
            let addr: IpAddr = ip_addr.to_be_bytes().into();

            peers.insert(Peer::new(addr, port));
            n -= 6;
        }

        trace!("Got peers: {:?}", peers);

        let resp = AnnounceResponse {
            interval: interval as u64,
            peers,
            peers6: hashset![],
            resolved_addr: Some(self.addr),
        };

        Ok(resp)
    }

    async fn read_response<'a>(
        &mut self,
        buf: &'a mut [u8],
        min_len: usize,
    ) -> crate::Result<(usize, Cursor<&'a [u8]>)> {
        let (len, addr) = self.socket.recv_from(buf).await?;

        if addr != self.addr {
            return Err("Packet received from unexpected address".into());
        }

        if len < min_len {
            return Err("Packet too small".into());
        }

        let buf = &buf[..len];

        let mut c = Cursor::new(buf);
        let action = c.read_u32::<BE>()?;
        let txn_id = c.read_u32::<BE>()?;

        trace!("Received action: {}, txn_id: {}", action, txn_id);

        if self.pending_action != action {
            return Err("Incorrect msg action received".into());
        }

        if self.txn_id != txn_id {
            return Err("Txn Id mismatch".into());
        }

        Ok((len, c))
    }

    fn write_connect(&mut self, buf: &mut [u8]) -> crate::Result<usize> {
        let mut c = Cursor::new(buf);
        c.write_u64::<BE>(TRACKER_CONSTANT)?;
        c.write_u32::<BE>(action::CONNECT)?;
        c.write_u32::<BE>(self.txn_id)?;
        Ok(c.position() as usize)
    }

    fn write_announce(&mut self, buf: &mut [u8]) -> crate::Result<usize> {
        let mut c = Cursor::new(buf);
        c.write_u64::<BE>(self.conn_id)?;
        c.write_u32::<BE>(action::ANNOUNCE)?;
        c.write_u32::<BE>(self.txn_id)?;
        c.write_all(self.req.info_hash.as_ref())?;
        c.write_all(&self.req.peer_id)?;
        c.write_u64::<BE>(0)?; // downloaded
        c.write_u64::<BE>(0)?; // left
        c.write_u64::<BE>(0)?; // uploaded
        c.write_u32::<BE>(self.req.event as u32)?;
        c.write_u32::<BE>(0)?; // IP addr
        c.write_u32::<BE>(0)?; // key
        c.write_i32::<BE>(-1)?; // num_want
        c.write_u16::<BE>(self.req.port)?; // port
        Ok(c.position() as usize)
    }
}

async fn resolve_addr(url: &str) -> crate::Result<SocketAddr> {
    let url: Url = url.parse().map_err(|_| "Failed to parse tracker url")?;
    if url.scheme() != "udp" {
        return Err("Not a UDP url".into());
    }

    let host = url.host_str().ok_or("Missing host")?;
    let port = url.port().ok_or("Missing port")?;

    for addr in lookup_host((host, port)).await? {
        trace!("Resolved {}/{} to {}", host, port, addr);
        return Ok(addr);
    }

    Err("Host/port is not resolved to a socket addr".into())
}
