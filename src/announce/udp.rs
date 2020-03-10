use crate::announce::{AnnounceRequest, AnnounceResponse};
use crate::peer::Peer;
use log::trace;
use rand::thread_rng;
use rand::Rng;
use std::io::Cursor;
use std::net::IpAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UdpSocket;

pub const TRACKER_CONSTANT: u64 = 0x41727101980;

mod action {
    pub const CONNECT: u32 = 0;
    pub const ANNOUNCE: u32 = 1;
}

pub async fn announce(req: AnnounceRequest<'_>) -> crate::Result<AnnounceResponse> {
    let mut c = UdpTrackerConnection::new(req.url).await?;
    c.connect().await?;
    c.announce(req).await
}

#[derive(Debug)]
struct UdpTrackerConnection {
    conn: UdpSocket,
    buf: Box<[u8]>,
    conn_id: u64,
    txn_id: u32,
}

impl UdpTrackerConnection {
    async fn new(url: &str) -> crate::Result<Self> {
        let url = url::Url::parse(url).map_err(|_| "Failed to parse tracker url")?;
        if url.scheme() != "udp" {
            return Err("Not a UDP url".into());
        }

        let host = url.host_str().ok_or("Missing host")?;
        let port = url.port().ok_or("Missing port")?;
        let conn = UdpSocket::bind("localhost:6881").await?;
        conn.connect((host, port)).await?;
        Ok(Self {
            conn,
            buf: vec![0; 4096].into_boxed_slice(),
            conn_id: 0,
            txn_id: 0,
        })
    }

    fn update_txn_id(&mut self) {
        self.txn_id = thread_rng().gen();
    }

    async fn connect(&mut self) -> crate::Result<()> {
        self.update_txn_id();
        let mut c = Cursor::new(&mut self.buf[..]);
        c.write_u64(TRACKER_CONSTANT).await?;
        c.write_u32(action::CONNECT).await?;
        c.write_u32(self.txn_id).await?;

        let n = self.conn.send(&self.buf[..16]).await?;
        if n != 16 {
            return Err("Error sending data".into());
        }

        let n = self.conn.recv(&mut self.buf[..]).await?;
        if n < 16 {
            return Err("Error receiving data".into());
        }

        let mut c = Cursor::new(&self.buf[..]);
        let action = c.read_u32().await?;
        if action != action::CONNECT {
            return Err("Expected CONNECT action".into());
        }

        let txn_id = c.read_u32().await?;
        if self.txn_id != txn_id {
            return Err("Incorrect transaction ID received".into());
        }

        let conn_id = c.read_u64().await?;
        self.conn_id = conn_id;
        self.update_txn_id();
        Ok(())
    }

    async fn announce(&mut self, req: AnnounceRequest<'_>) -> crate::Result<AnnounceResponse> {
        let mut c = Cursor::new(&mut self.buf[..]);
        c.write_u64(self.conn_id).await?;
        c.write_u32(action::ANNOUNCE).await?;
        c.write_u32(self.txn_id).await?;
        c.write_all(req.info_hash.as_ref()).await?;
        c.write_all(req.peer_id).await?;
        c.write_u64(0).await?; // downloaded
        c.write_u64(0).await?; // left
        c.write_u64(0).await?; // uploaded
        c.write_u32(req.event as u32).await?;
        c.write_u32(0).await?; // IP addr
        c.write_u32(0).await?; // key
        c.write_i32(-1).await?; // num_want
        c.write_u16(req.port).await?; // port

        let n = self.conn.send(&self.buf[..98]).await?;
        if n != 98 {
            return Err("Error sending data".into());
        }

        let mut n = self.conn.recv(&mut self.buf[..]).await?;
        if n < 20 {
            return Err("Error receiving data".into());
        }

        let mut c = Cursor::new(&self.buf[..]);
        let action = c.read_u32().await?;
        if action != action::ANNOUNCE {
            return Err("Expected ANNOUNCE action".into());
        }

        let txn_id = c.read_u32().await?;
        if self.txn_id != txn_id {
            return Err("Incorrect transaction ID received".into());
        }

        let interval = c.read_u32().await?;
        trace!("interval: {}", interval);

        let leechers = c.read_u32().await?;
        trace!("leechers: {}", leechers);

        let seeders = c.read_u32().await?;
        trace!("seeders: {}", seeders);

        n -= 20;

        if n % 6 != 0 {
            return Err("IPs should be 6 byte each".into());
        }

        let mut peers = vec![];
        while n > 0 {
            let ip_addr = c.read_u32().await?;
            let port = c.read_u16().await?;
            trace!("Addr: {}, port: {}", ip_addr, port);
            let addr: IpAddr = ip_addr.to_be_bytes().into();
            peers.push(Peer::new(addr, port));
            n -= 6;
        }
        Ok(AnnounceResponse {
            interval: interval as usize,
            peers,
            peers6: vec![],
        })
    }
}
