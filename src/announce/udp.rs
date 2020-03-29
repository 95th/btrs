use crate::announce::{AnnounceRequest, AnnounceResponse};
use crate::peer::Peer;
use futures::channel::{mpsc, oneshot};
use futures::{SinkExt, StreamExt};
use log::{trace, warn};
use rand::thread_rng;
use rand::Rng;
use std::collections::HashMap;
use std::io::Cursor;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{lookup_host, UdpSocket};

const TRACKER_CONSTANT: u64 = 0x0417_2710_1980;
const TRACKER_TIMEOUT: Duration = Duration::from_secs(10);

mod action {
    pub const CONNECT: u32 = 0;
    pub const ANNOUNCE: u32 = 1;
}

type AnnounceResponseTx = oneshot::Sender<crate::Result<AnnounceResponse>>;

struct Tracker {
    addr: SocketAddr,
    req: AnnounceRequest,
    tx: AnnounceResponseTx,
    last_updated: Instant,
    conn_id: u64,
    txn_id: u32,
    pending_action: u32,
}

impl Tracker {
    async fn new(
        req: AnnounceRequest,
        tx: AnnounceResponseTx,
        socket: &mut UdpSocket,
        buf: &mut [u8],
    ) -> Option<Tracker> {
        let f = async {
            let url = url::Url::parse(&req.url).map_err(|_| "Failed to parse tracker url")?;
            if url.scheme() != "udp" {
                return Err("Not a UDP url".into());
            }

            let host = url.host_str().ok_or("Missing host")?;
            let port = url.port().ok_or("Missing port")?;

            let addr = lookup_host((host, port))
                .await?
                .next()
                .ok_or("Host/port is not resolved to a socket addr")?;
            Ok(addr)
        };

        let addr = match f.await {
            Ok(x) => x,
            Err(e) => {
                let _ = tx.send(Err(e));
                return None;
            }
        };

        let mut c = Tracker {
            addr,
            req,
            tx,
            last_updated: Instant::now(),
            conn_id: 0,
            txn_id: 0,
            pending_action: 0,
        };

        match c.send_connect(socket, buf).await {
            Ok(()) => Some(c),
            Err(e) => {
                let _ = c.tx.send(Err(e));
                None
            }
        }
    }

    fn update_txn_id(&mut self) {
        self.txn_id = thread_rng().gen();
    }

    async fn send_connect(&mut self, socket: &mut UdpSocket, buf: &mut [u8]) -> crate::Result<()> {
        self.update_txn_id();
        let mut c = Cursor::new(&mut *buf);
        c.write_u64(TRACKER_CONSTANT).await?;
        c.write_u32(action::CONNECT).await?;
        c.write_u32(self.txn_id).await?;

        let n = socket.send_to(&buf[..16], &self.addr).await?;
        if n != 16 {
            return Err("Error sending data".into());
        }

        self.pending_action = action::CONNECT;
        self.last_updated = Instant::now();
        Ok(())
    }

    async fn send_announce(&mut self, socket: &mut UdpSocket, buf: &mut [u8]) -> crate::Result<()> {
        let mut c = Cursor::new(&mut *buf);
        c.write_u64(self.conn_id).await?;
        c.write_u32(action::ANNOUNCE).await?;
        c.write_u32(self.txn_id).await?;
        c.write_all(self.req.info_hash.as_ref()).await?;
        c.write_all(&self.req.peer_id).await?;
        c.write_u64(0).await?; // downloaded
        c.write_u64(0).await?; // left
        c.write_u64(0).await?; // uploaded
        c.write_u32(self.req.event as u32).await?;
        c.write_u32(0).await?; // IP addr
        c.write_u32(0).await?; // key
        c.write_i32(-1).await?; // num_want
        c.write_u16(self.req.port).await?; // port

        let n = socket.send_to(&buf[..98], &self.addr).await?;
        if n != 98 {
            return Err("Error sending data".into());
        }

        self.pending_action = action::ANNOUNCE;
        self.last_updated = Instant::now();
        Ok(())
    }
}

pub struct UdpTrackerMgr {
    rx: mpsc::Receiver<(AnnounceRequest, AnnounceResponseTx)>,
    tx: mpsc::Sender<(AnnounceRequest, AnnounceResponseTx)>,
    socket: UdpSocket,
}

#[derive(Clone)]
pub struct UdpTrackerMgrHandle(mpsc::Sender<(AnnounceRequest, AnnounceResponseTx)>);

impl UdpTrackerMgrHandle {
    pub async fn announce(&mut self, req: AnnounceRequest) -> crate::Result<AnnounceResponse> {
        let (tx, rx) = oneshot::channel();
        self.0
            .send((req, tx))
            .await
            .map_err(|_| "Unable to send announce request")?;
        rx.await
            .map_err(|_| "Unable to receive announce response")?
    }
}

impl UdpTrackerMgr {
    pub async fn new() -> crate::Result<UdpTrackerMgr> {
        let socket = UdpSocket::bind(("localhost", 6882)).await?;
        let (tx, rx) = mpsc::channel(100);
        Ok(UdpTrackerMgr { socket, tx, rx })
    }

    pub fn handle(&self) -> UdpTrackerMgrHandle {
        UdpTrackerMgrHandle(self.tx.clone())
    }

    pub async fn listen(&mut self) {
        trace!("Listening for announce requests");

        let mut pending = HashMap::new();

        let rx = &mut self.rx;
        let socket = &mut self.socket;
        let mut buf = [0; 4096];

        let mut channel_open = true;

        loop {
            if channel_open && pending.is_empty() {
                // Wait for requests
                match rx.next().await {
                    Some((req, tx)) => {
                        trace!("Got an announce request");
                        let tc = Tracker::new(req, tx, socket, &mut buf).await;
                        if let Some(tc) = tc {
                            pending.insert(tc.addr, tc);
                        }
                    }
                    None => break,
                }
            }

            // Read as many requests as we can without blocking (well blocking only to write
            // connects to socket which shouldn't block much)
            loop {
                match rx.try_next() {
                    Ok(Some((req, tx))) => {
                        trace!("Got an announce request");
                        let tc = Tracker::new(req, tx, socket, &mut buf).await;
                        if let Some(tc) = tc {
                            pending.insert(tc.addr, tc);
                        }
                    }
                    Ok(None) => {
                        channel_open = false;
                        break;
                    }
                    Err(_) => {
                        channel_open = true;
                        break;
                    }
                }
            }

            trace!("channel_open: {}, pending: {}", channel_open, pending.len(),);

            if !channel_open && pending.is_empty() {
                // Channel is closed and no pending items - We're done
                break;
            }

            let f = async {
                let (mut n, addr) = socket.recv_from(&mut buf[..]).await?;
                let mut tc = pending
                    .remove(&addr)
                    .ok_or("Msg received from unexpected addr")?;

                if n < 8 {
                    return Err("Packet too small".into());
                }

                let mut c = Cursor::new(&buf[..]);
                let action = c.read_u32().await?;
                let txn_id = c.read_u32().await?;

                trace!("action: {}, txn_id: {}", action, txn_id);

                if tc.pending_action != action {
                    return Err("Incorrect msg action received".into());
                }

                if action == action::CONNECT {
                    if n < 16 {
                        return Err("Packet too small".into());
                    }

                    if tc.txn_id != txn_id {
                        return Err("Txn Id mismatch".into());
                    }

                    let conn_id = c.read_u64().await?;
                    trace!("conn_id: {}", conn_id);

                    tc.conn_id = conn_id;
                    tc.update_txn_id();

                    tc.send_announce(socket, &mut buf).await?;
                    trace!("sent announce");

                    pending.insert(tc.addr, tc);
                } else if action == action::ANNOUNCE {
                    if n < 20 {
                        return Err("Packet too small".into());
                    }

                    if tc.txn_id != txn_id {
                        return Err("Txn Id mismatch".into());
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

                    let mut peers = hashset![];
                    while n > 0 {
                        let ip_addr = c.read_u32().await?;
                        let port = c.read_u16().await?;
                        trace!("Addr: {}, port: {}", ip_addr, port);
                        let addr: IpAddr = ip_addr.to_be_bytes().into();
                        peers.insert(Peer::new(addr, port));
                        n -= 6;
                    }
                    let resp = AnnounceResponse {
                        interval: interval as u64,
                        peers,
                        peers6: hashset![],
                    };

                    let _ = tc.tx.send(Ok(resp));
                } else {
                    return Err("Invalid action received".into());
                }

                Ok::<_, crate::Error>(())
            };

            if let Err(e) = f.await {
                warn!("Error: {}", e);
            }

            trace!("Before culling: pending: {}", pending.len(),);

            let cutoff = Instant::now() - TRACKER_TIMEOUT;

            // Cull timed out entries
            pending.retain(|_, tc| tc.last_updated > cutoff);

            trace!("After culling: pending: {}", pending.len(),);
        }
    }
}
