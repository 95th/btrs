use crate::announce::{AnnounceRequest, AnnounceResponse};
use crate::peer::Peer;
use futures::channel::{mpsc, oneshot};
use futures::StreamExt;
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

struct Tracker<'a> {
    addr: SocketAddr,
    req: AnnounceRequest<'a>,
    tx: oneshot::Sender<crate::Result<AnnounceResponse>>,
    added: Instant,
    conn_id: u64,
    txn_id: u32,
}

impl Tracker<'_> {
    async fn new<'a>(
        req: AnnounceRequest<'a>,
        tx: oneshot::Sender<crate::Result<AnnounceResponse>>,
        conn: &mut UdpSocket,
        buf: &mut [u8],
    ) -> Option<Tracker<'a>> {
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
            added: Instant::now(),
            conn_id: 0,
            txn_id: 0,
        };

        match c.send_connect(conn, buf).await {
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

    async fn send_connect(&mut self, conn: &mut UdpSocket, buf: &mut [u8]) -> crate::Result<()> {
        self.update_txn_id();
        let mut c = Cursor::new(&mut *buf);
        c.write_u64(TRACKER_CONSTANT).await?;
        c.write_u32(action::CONNECT).await?;
        c.write_u32(self.txn_id).await?;

        let n = conn.send_to(&buf[..16], &self.addr).await?;
        if n != 16 {
            return Err("Error sending data".into());
        }

        Ok(())
    }

    async fn send_announce(&mut self, conn: &mut UdpSocket, buf: &mut [u8]) -> crate::Result<()> {
        let mut c = Cursor::new(&mut *buf);
        c.write_u64(self.conn_id).await?;
        c.write_u32(action::ANNOUNCE).await?;
        c.write_u32(self.txn_id).await?;
        c.write_all(self.req.info_hash.as_ref()).await?;
        c.write_all(self.req.peer_id).await?;
        c.write_u64(0).await?; // downloaded
        c.write_u64(0).await?; // left
        c.write_u64(0).await?; // uploaded
        c.write_u32(self.req.event as u32).await?;
        c.write_u32(0).await?; // IP addr
        c.write_u32(0).await?; // key
        c.write_i32(-1).await?; // num_want
        c.write_u16(self.req.port).await?; // port

        let n = conn.send_to(&buf[..98], &self.addr).await?;
        if n != 98 {
            return Err("Error sending data".into());
        }

        Ok(())
    }
}

pub struct TrackerManager<'a> {
    request_rx: mpsc::Receiver<(
        AnnounceRequest<'a>,
        oneshot::Sender<crate::Result<AnnounceResponse>>,
    )>,
    request_tx: mpsc::Sender<(
        AnnounceRequest<'a>,
        oneshot::Sender<crate::Result<AnnounceResponse>>,
    )>,
    udp_socket: UdpSocket,
}

pub struct TrackerManagerHandle<'a>(
    mpsc::Sender<(
        AnnounceRequest<'a>,
        oneshot::Sender<crate::Result<AnnounceResponse>>,
    )>,
);

impl<'a> TrackerManagerHandle<'a> {
    // pub fn announce(&mut self, req: AnnounceRequest<'a>) -> crate::Result<AnnounceResponse> {
    //     self.0.send(req);
    // }
}

impl<'a> TrackerManager<'a> {
    pub async fn new() -> crate::Result<TrackerManager<'a>> {
        let udp_socket = UdpSocket::bind(("localhost", 6881)).await?;
        let (request_tx, request_rx) = mpsc::channel(100);
        Ok(TrackerManager {
            udp_socket,
            request_tx,
            request_rx,
        })
    }

    pub fn handle(&self) -> TrackerManagerHandle<'a> {
        TrackerManagerHandle(self.request_tx.clone())
    }

    pub async fn listen(&mut self) {
        let mut pending_connect = HashMap::new();
        let mut pending_announce = HashMap::new();

        let request_rx = &mut self.request_rx;
        let udp_socket = &mut self.udp_socket;
        let mut buf = [0; 4096];

        let mut pending_rx = true;

        loop {
            if pending_rx && pending_announce.is_empty() && pending_connect.is_empty() {
                match request_rx.next().await {
                    Some((req, tx)) => {
                        let tc = Tracker::new(req, tx, udp_socket, &mut buf).await;
                        if let Some(tc) = tc {
                            pending_connect.insert(tc.txn_id, tc);
                        }
                    }
                    None => break,
                }
            } else {
                loop {
                    match request_rx.try_next() {
                        Ok(Some((req, tx))) => {
                            let tc = Tracker::new(req, tx, udp_socket, &mut buf).await;
                            if let Some(tc) = tc {
                                pending_connect.insert(tc.txn_id, tc);
                            }
                        }
                        Ok(None) => {
                            pending_rx = false;
                            break;
                        }
                        Err(_) => {
                            pending_rx = true;
                            break;
                        }
                    }
                }
            }

            if !pending_rx && pending_connect.is_empty() && pending_announce.is_empty() {
                // Channel is closed and no pending items - We're done
                break;
            }

            let f = async {
                let (mut n, addr) = udp_socket.recv_from(&mut buf[..]).await?;

                if n < 4 {
                    return Err("Packet too small".into());
                }

                let mut c = Cursor::new(&buf[..]);
                let action = c.read_u32().await?;
                let txn_id = c.read_u32().await?;

                if action == action::CONNECT {
                    if n < 16 {
                        return Err("Packet too small".into());
                    }

                    let mut tc = pending_connect.remove(&txn_id).ok_or("Unknown txn id")?;
                    if tc.addr != addr {
                        return Err("Address mismatch".into());
                    }

                    let conn_id = c.read_u64().await?;
                    tc.conn_id = conn_id;
                    tc.update_txn_id();

                    tc.send_announce(udp_socket, &mut buf).await?;
                    pending_announce.insert(tc.txn_id, tc);
                } else if action == action::ANNOUNCE {
                    if n < 20 {
                        return Err("Packet too small".into());
                    }

                    let tc = pending_announce.remove(&txn_id).ok_or("Unknown txn id")?;
                    if tc.addr != addr {
                        return Err("Address mismatch".into());
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

            let now = Instant::now();

            // Cull timed out entries
            pending_connect.retain(|_, tc| tc.added + TRACKER_TIMEOUT < now);
            pending_announce.retain(|_, tc| tc.added + TRACKER_TIMEOUT < now);
        }
    }
}
