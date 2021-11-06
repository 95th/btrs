use crate::announce::{AnnounceRequest, AnnounceResponse};
use crate::peer::Peer;
use anyhow::Context;
use byteorder::{ReadBytesExt, WriteBytesExt, BE};
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

pub async fn announce(
    req: AnnounceRequest<'_>,
    buf: &mut [u8],
) -> anyhow::Result<AnnounceResponse> {
    let mut t = UdpTracker::new(req).await?;
    t.connect(buf).await?;
    t.announce(buf).await
}

struct UdpTracker<'a> {
    socket: UdpSocket,
    addr: SocketAddr,
    req: AnnounceRequest<'a>,
    conn_id: u64,
    txn_id: u32,
}

impl<'a> UdpTracker<'a> {
    pub async fn new(req: AnnounceRequest<'a>) -> anyhow::Result<UdpTracker<'a>> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
        let addr = match req.resolved_addr {
            Some(a) => a,
            None => resolve_addr(req.url).await?,
        };

        Ok(UdpTracker {
            socket,
            addr,
            req,
            conn_id: 0,
            txn_id: 0,
        })
    }

    fn update_txn_id(&mut self) {
        self.txn_id = thread_rng().gen();
    }

    async fn connect(&mut self, buf: &mut [u8]) -> anyhow::Result<()> {
        self.update_txn_id();

        trace!("Sending connect to {}, txn id: {}", self.addr, self.txn_id);

        let n = self.write_connect(buf)?;
        let written = self.socket.send_to(&buf[..n], &self.addr).await?;
        anyhow::ensure!(written == n, "Error sending data");

        let (_, mut c) = self.read_response(action::CONNECT, buf, 16).await?;
        let conn_id = c.read_u64::<BE>()?;
        trace!("conn_id: {}", conn_id);
        self.conn_id = conn_id;

        Ok(())
    }

    async fn announce(&mut self, buf: &mut [u8]) -> anyhow::Result<AnnounceResponse> {
        self.update_txn_id();

        trace!("Sending announce to {}, txn id: {}", self.addr, self.txn_id);

        let n = self.write_announce(buf)?;
        let written = self.socket.send_to(&buf[..n], &self.addr).await?;
        anyhow::ensure!(written == n, "Error sending data");

        let (len, mut c) = self.read_response(action::ANNOUNCE, buf, 20).await?;

        let interval = c.read_u32::<BE>()?;
        let leechers = c.read_u32::<BE>()?;
        let seeders = c.read_u32::<BE>()?;

        trace!("interval: {}", interval);
        trace!("seeders: {}", seeders);
        trace!("leechers: {}", leechers);

        let mut n = len - 20;
        anyhow::ensure!(n % 6 == 0, "IPs should be 6 byte each");

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

    async fn read_response<'b>(
        &self,
        expected_action: u32,
        buf: &'b mut [u8],
        min_len: usize,
    ) -> anyhow::Result<(usize, Cursor<&'b [u8]>)> {
        let (len, addr) = self.socket.recv_from(buf).await?;

        anyhow::ensure!(addr == self.addr, "Packet received from unexpected address");
        anyhow::ensure!(len >= min_len, "Packet too small");

        let buf = &buf[..len];

        let mut c = Cursor::new(buf);
        let action = c.read_u32::<BE>()?;
        let txn_id = c.read_u32::<BE>()?;

        trace!("Received action: {}, txn_id: {}", action, txn_id);

        anyhow::ensure!(expected_action == action, "Incorrect msg action received");
        anyhow::ensure!(self.txn_id == txn_id, "Txn Id mismatch");

        Ok((len, c))
    }

    fn write_connect(&self, buf: &mut [u8]) -> anyhow::Result<usize> {
        let mut c = Cursor::new(buf);
        c.write_u64::<BE>(TRACKER_CONSTANT)?;
        c.write_u32::<BE>(action::CONNECT)?;
        c.write_u32::<BE>(self.txn_id)?;
        Ok(c.position() as usize)
    }

    fn write_announce(&self, buf: &mut [u8]) -> anyhow::Result<usize> {
        let mut c = Cursor::new(buf);
        c.write_u64::<BE>(self.conn_id)?;
        c.write_u32::<BE>(action::ANNOUNCE)?;
        c.write_u32::<BE>(self.txn_id)?;
        c.write_all(self.req.info_hash.as_ref())?;
        c.write_all(&self.req.peer_id[..])?;
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

async fn resolve_addr(url: &str) -> anyhow::Result<SocketAddr> {
    let url: Url = url.parse().context("Failed to parse tracker url")?;
    anyhow::ensure!(url.scheme() == "udp", "Not a UDP url");

    let host = url.host_str().context("Missing host")?;
    let port = url.port().context("Missing port")?;

    let mut result = lookup_host((host, port)).await?;
    if let Some(addr) = result.next() {
        trace!("Resolved {}/{} to {}", host, port, addr);
        Ok(addr)
    } else {
        anyhow::bail!("Host/port is not resolved to a socket addr")
    }
}
