use crate::announce::{AnnounceRequest, AnnounceResponse};
use crate::future::timeout;
use crate::peer::Peer;
use byteorder::{ReadBytesExt, WriteBytesExt, BE};
use futures::channel::{mpsc, oneshot};
use futures::{SinkExt, StreamExt};
use log::{trace, warn};
use rand::thread_rng;
use rand::Rng;
use std::collections::HashMap;
use std::io::Cursor;
use std::io::Write;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};
use tokio::net::{lookup_host, UdpSocket};
use url::Url;

const TRACKER_CONSTANT: u64 = 0x0417_2710_1980;
const TRACKER_TIMEOUT: Duration = Duration::from_secs(10);

mod action {
    pub const CONNECT: u32 = 0;
    pub const ANNOUNCE: u32 = 1;
}

type AnnounceResponseTx = oneshot::Sender<crate::Result<AnnounceResponse>>;

struct UdpTracker {
    addr: SocketAddr,
    req: AnnounceRequest,
    tx: AnnounceResponseTx,
    last_updated: Instant,
    conn_id: u64,
    txn_id: u32,
    pending_action: u32,
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

impl UdpTracker {
    async fn new(
        req: AnnounceRequest,
        tx: AnnounceResponseTx,
        socket: &mut UdpSocket,
        buf: &mut [u8],
    ) -> Option<UdpTracker> {
        let addr = match req.resolved_addr {
            Some(addr) => addr,
            None => match resolve_addr(&req.url).await {
                Ok(addr) => addr,
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return None;
                }
            },
        };

        let mut c = UdpTracker {
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

        trace!("Sending connect to {}, txn id: {}", self.addr, self.txn_id);

        let mut c = Cursor::new(&mut *buf);
        c.write_u64::<BE>(TRACKER_CONSTANT)?;
        c.write_u32::<BE>(action::CONNECT)?;
        c.write_u32::<BE>(self.txn_id)?;

        let n = socket.send_to(&buf[..16], &self.addr).await?;
        if n != 16 {
            return Err("Error sending data".into());
        }

        self.pending_action = action::CONNECT;
        self.last_updated = Instant::now();
        Ok(())
    }

    async fn send_announce(&mut self, socket: &mut UdpSocket, buf: &mut [u8]) -> crate::Result<()> {
        self.update_txn_id();

        trace!("Sending announce to {}, txn id: {}", self.addr, self.txn_id);

        let mut c = Cursor::new(&mut *buf);
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

        let n = socket.send_to(&buf[..98], &self.addr).await?;
        if n != 98 {
            return Err("Error sending data".into());
        }

        self.pending_action = action::ANNOUNCE;
        self.last_updated = Instant::now();
        Ok(())
    }

    fn handle_response(mut self, buf: &[u8]) -> crate::Result<Option<Self>> {
        if buf.len() < 16 {
            return Err("Packet too small".into());
        }

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

        if action == action::CONNECT {
            let conn_id = c.read_u64::<BE>()?;
            trace!("conn_id: {}", conn_id);

            self.conn_id = conn_id;

            Ok(Some(self))
        } else if action == action::ANNOUNCE {
            if buf.len() < 20 {
                return Err("Packet too small".into());
            }

            let interval = c.read_u32::<BE>()?;
            let leechers = c.read_u32::<BE>()?;
            let seeders = c.read_u32::<BE>()?;

            trace!("interval: {}", interval);
            trace!("seeders: {}", seeders);
            trace!("leechers: {}", leechers);

            let mut n = buf.len() - 20;

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

            let _ = self.tx.send(Ok(resp));
            Ok(None)
        } else {
            Err("Invalid action received".into())
        }
    }
}

pub struct UdpTrackerMgr {
    rx: mpsc::Receiver<(AnnounceRequest, AnnounceResponseTx)>,
    tx: mpsc::Sender<(AnnounceRequest, AnnounceResponseTx)>,
    socket: UdpSocket,
    pending: HashMap<SocketAddr, UdpTracker>,
    channel_open: bool,
}

#[derive(Clone)]
pub struct UdpTrackerMgrHandle(mpsc::Sender<(AnnounceRequest, AnnounceResponseTx)>);

impl UdpTrackerMgrHandle {
    pub async fn announce(&mut self, req: AnnounceRequest) -> crate::Result<AnnounceResponse> {
        let (tx, rx) = oneshot::channel();
        self.0.send((req, tx)).await.unwrap();
        rx.await.unwrap()
    }
}

impl UdpTrackerMgr {
    pub async fn new() -> crate::Result<UdpTrackerMgr> {
        let socket = UdpSocket::bind(("localhost", 6882)).await?;
        let (tx, rx) = mpsc::channel(100);
        Ok(UdpTrackerMgr {
            socket,
            tx,
            rx,
            pending: HashMap::new(),
            channel_open: true,
        })
    }

    pub fn handle(&self) -> UdpTrackerMgrHandle {
        UdpTrackerMgrHandle(self.tx.clone())
    }

    pub async fn listen(&mut self) {
        trace!("Listening for announce requests");

        let mut buf = [0; 4096];

        while self.has_work() {
            self.add_requests(&mut buf).await;
            self.handle_requests(&mut buf).await;
        }
    }

    fn has_work(&self) -> bool {
        self.channel_open || !self.pending.is_empty()
    }

    async fn add_requests(&mut self, buf: &mut [u8]) {
        if !self.channel_open {
            return;
        }

        if self.pending.is_empty() {
            // Wait for a request
            let (req, tx) = match self.rx.next().await {
                Some(r) => r,
                None => {
                    self.channel_open = false;
                    return;
                }
            };

            trace!("Got an announce request");
            let tracker = UdpTracker::new(req, tx, &mut self.socket, buf).await;
            if let Some(tracker) = tracker {
                self.pending.insert(tracker.addr, tracker);
            }
        }

        // Read as many requests as we can without blocking on request channel
        loop {
            let (req, tx) = match self.rx.try_next() {
                Ok(Some(r)) => r,
                Ok(None) => {
                    self.channel_open = false;
                    break;
                }
                Err(_) => break,
            };

            trace!("Got an announce request");
            let tracker = UdpTracker::new(req, tx, &mut self.socket, buf).await;
            if let Some(tracker) = tracker {
                self.pending.insert(tracker.addr, tracker);
            }
        }

        trace!(
            "channel_open: {}, pending: {}",
            self.channel_open,
            self.pending.len(),
        );
    }

    async fn handle_requests(&mut self, buf: &mut [u8]) {
        if self.pending.is_empty() {
            return;
        }

        if let Err(e) = timeout(self.process_response(buf), 3).await {
            warn!("Error: {}", e);
        }

        // Cull timed out entries
        let cutoff = Instant::now() - TRACKER_TIMEOUT;
        self.pending.retain(|_, t| t.last_updated > cutoff);

        trace!("Pending UDP trackers: {}", self.pending.len());
    }

    async fn process_response(&mut self, buf: &mut [u8]) -> crate::Result<()> {
        let (len, addr) = self.socket.recv_from(buf).await?;
        let tracker = self
            .pending
            .remove(&addr)
            .ok_or("Msg received from unexpected addr")?;

        if let Some(mut tracker) = tracker.handle_response(&buf[..len])? {
            tracker.send_announce(&mut self.socket, buf).await?;
            self.pending.insert(tracker.addr, tracker);
        }
        Ok(())
    }
}
