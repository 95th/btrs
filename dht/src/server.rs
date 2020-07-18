use crate::bucket::Bucket;
use crate::contact::{CompactNodes, CompactNodesV6, ContactStatus};
use crate::id::NodeId;
use crate::msg::{AnnouncePeer, FindNode, Msg, MsgKind, TxnId};
use crate::table::RoutingTable;
use anyhow::Context as _;
use ben::{Decoder, Encode, Parser};
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::mpsc::{self, Receiver, Sender};

pub struct Server {
    rpc: RpcMgr,
    table: RoutingTable,
    txn_id: TxnId,
    own_id: NodeId,
    txns: Transactions,
    router_nodes: Vec<SocketAddr>,
    next_refresh: Instant,
    client_rx: Receiver<ClientRequest>,
    client_tx: Sender<ClientRequest>,
}

#[derive(Clone)]
pub struct Client {
    pub tx: Sender<ClientRequest>,
}

#[derive(Debug)]
pub enum ClientRequest {
    Announce(NodeId),
    Shutdown,
}

impl Server {
    pub async fn new(port: u16, router_nodes: Vec<SocketAddr>) -> anyhow::Result<Server> {
        let addr = SocketAddr::from(([0u8; 4], port));
        let socket = UdpSocket::bind(addr).await?;
        let id = NodeId::gen();
        let (client_tx, client_rx) = mpsc::channel(100);

        let server = Server {
            rpc: RpcMgr::new(socket),
            table: RoutingTable::new(id),
            txn_id: TxnId(0),
            own_id: id,
            txns: Transactions::new(),
            router_nodes,
            next_refresh: Instant::now(),
            client_tx,
            client_rx,
        };

        Ok(server)
    }

    pub fn new_client(&self) -> Client {
        Client {
            tx: self.client_tx.clone(),
        }
    }

    pub async fn run(mut self) {
        loop {
            if Instant::now() >= self.next_refresh {
                // refresh the table
                if self.table.is_empty() {
                    let target = self.own_id;
                    self.refresh(&target).await;
                } else if let Some(target) = self.table.pick_refresh_id() {
                    trace!("Bucket refresh target: {:?}", target);
                    self.refresh(&target).await;
                }

                // Self refresh every 15 mins
                self.next_refresh = Instant::now() + Duration::from_secs(15 * 60);
            }

            // Check if any request from client such as Announce/Shutdown
            if self.check_client_request().await {
                debug!("Shutdown received from client");
                // TODO: Save DHT state on disk
                break;
            }

            // Wait for socket response
            self.recv_response(Duration::from_millis(100)).await;

            // Clear stale transactions
            self.txns.prune(&mut self.table);
        }
    }

    async fn refresh(&mut self, target: &NodeId) {
        let mut nodes = VecDeque::new();
        let mut min_dist = NodeId::max();

        if !self.table.is_empty() {
            let mut closest = Vec::with_capacity(Bucket::MAX_LEN);
            self.table
                .find_closest(target, &mut closest, Bucket::MAX_LEN);

            for c in closest {
                nodes.push_front(c.addr);
                let dist = target ^ c.id;
                min_dist = min_dist.min(dist);
            }
        }

        if nodes.len() < 3 {
            for addr in &self.router_nodes {
                nodes.push_back(*addr);
            }
        }

        debug!("Start refresh with {} nodes", nodes.len());

        let max_outstanding = 3;

        loop {
            while self.txns.len() < max_outstanding {
                if let Some(node) = nodes.pop_front() {
                    self.find_node(target, &node).await;
                } else {
                    break;
                }
            }

            self.txns.prune(&mut self.table);
            debug!("Pending requests: {}", self.txns.len());

            if self.txns.is_empty() {
                trace!("Done bootstrapping. Min dist: {:?}", min_dist);
                debug!(
                    "Table size:: live: {}, extra: {}",
                    self.table.len(),
                    self.table.len_extra()
                );
                break;
            }

            let (msg, _) = match self.rpc.recv_timeout(Duration::from_secs(1)).await {
                Ok(Some(x)) => x,
                Ok(None) => continue,
                Err(e) => {
                    warn!("{}", e);
                    continue;
                }
            };

            self.table.handle_msg(msg, &mut self.txns);

            let mut closest = Vec::with_capacity(Bucket::MAX_LEN);
            self.table
                .find_closest(target, &mut closest, Bucket::MAX_LEN);

            let old_dist = min_dist;

            nodes.clear();
            for c in closest {
                let dist = target ^ c.id;
                if dist < old_dist {
                    nodes.push_front(c.addr);
                    min_dist = dist;
                }
            }
        }
    }

    async fn find_node(&mut self, target: &NodeId, addr: &SocketAddr) -> bool {
        debug!("Send FIND_NODE request to {}", addr);

        let m = FindNode {
            id: &self.own_id,
            target,
            txn_id: self.txn_id.next_id(),
        };
        match self.rpc.send(&m, addr).await {
            Ok(_) => {
                self.txns.insert(m.txn_id, *addr);
                true
            }
            Err(e) => {
                warn!("FIND_NODE to {} failed: {}", addr, e);
                false
            }
        }
    }

    async fn check_client_request(&mut self) -> bool {
        let info_hash = match self.client_rx.try_recv() {
            Ok(ClientRequest::Announce(infohash)) => infohash,
            Ok(ClientRequest::Shutdown) => return true,
            Err(_) => return false,
        };

        let mut closest = Vec::with_capacity(Bucket::MAX_LEN);
        self.table
            .find_closest(&info_hash, &mut closest, Bucket::MAX_LEN);

        let closest: Vec<_> = closest.into_iter().map(|c| (c.addr, *c.id)).collect();

        for c in closest {
            self.announce(&info_hash, &c.0, &c.1).await;
        }

        false
    }

    async fn announce(&mut self, info_hash: &NodeId, addr: &SocketAddr, id: &NodeId) -> bool {
        debug!("Send ANNOUNCE_PEER request to {}", addr);

        let token = match self.table.tokens.get(id) {
            Some(t) => t,
            None => {
                debug!("Token not found for {:?}", id);
                return false;
            }
        };

        let m = AnnouncePeer {
            id: &self.own_id,
            info_hash,
            txn_id: self.txn_id.next_id(),
            implied_port: true,
            port: 0,
            token,
        };

        match self.rpc.send(&m, addr).await {
            Ok(_) => {
                self.txns.insert(m.txn_id, *addr);
                true
            }
            Err(e) => {
                warn!("ANNOUNCE_PEER to {} failed: {}", addr, e);
                false
            }
        }
    }

    async fn recv_response(&mut self, timeout: Duration) {
        match self.rpc.recv_timeout(timeout).await {
            Ok(Some((msg, _addr))) => self.table.handle_msg(msg, &mut self.txns),
            Ok(None) => {}
            Err(e) => warn!("{}", e),
        }
    }
}

impl RoutingTable {
    fn handle_msg(&mut self, msg: Msg, txns: &mut Transactions) {
        if let MsgKind::Response = msg.kind {
            match txns.remove(&msg.txn_id) {
                Some(addr) => {
                    if let Some(c) = self.find_contact(&addr) {
                        c.status = ContactStatus::ALIVE | ContactStatus::QUERIED;
                        c.clear_timeout();
                        c.last_updated = Instant::now();
                    }
                }
                None => {
                    warn!(
                        "Message received (txn id: {:?}) from unexpected address",
                        msg.txn_id
                    );
                    return;
                }
            };

            self.handle_response(&msg);
            return;
        }

        if let MsgKind::Error = msg.kind {
            warn!("{:?}", msg);
            return;
        }

        self.handle_query(&msg);
    }

    fn handle_query(&mut self, msg: &Msg) {
        debug!("Got query request: {:#?}", msg);
    }

    fn handle_response(&mut self, msg: &Msg) {
        if let Err(e) = self.read_nodes(msg) {
            warn!("{}", e);
        }
    }

    fn read_nodes(&mut self, msg: &Msg) -> anyhow::Result<()> {
        if let MsgKind::Response = msg.kind {
            let resp = msg.body.get_dict(b"r").context("Response dict expected")?;

            let nodes = resp.get_bytes(b"nodes").context("nodes required")?;
            for c in CompactNodes::new(nodes)? {
                self.add_contact(&c);
            }

            if let Some(nodes6) = resp.get_bytes(b"nodes6") {
                for c in CompactNodesV6::new(nodes6)? {
                    self.add_contact(&c);
                }
            }

            if let Some(id) = msg.id {
                if let Some(token) = resp.get_bytes(b"token") {
                    self.tokens.insert(*id, token.to_vec());
                }
            }
        }
        Ok(())
    }
}

struct RpcMgr {
    socket: UdpSocket,
    buf: Vec<u8>,
    recv_buf: Box<[u8]>,
    parser: Parser,
}

impl RpcMgr {
    fn new(socket: UdpSocket) -> Self {
        Self {
            socket,
            buf: Vec::new(),
            recv_buf: vec![0; 2048].into_boxed_slice(),
            parser: Parser::new(),
        }
    }

    async fn send<E: Encode>(&mut self, msg: E, addr: &SocketAddr) -> anyhow::Result<()> {
        self.buf.clear();
        msg.encode(&mut self.buf);

        trace!(
            "Sending: {:?}",
            self.parser.parse::<Decoder>(&self.buf).unwrap()
        );

        let n = self.socket.send_to(&self.buf, addr).await?;
        trace!("Sent: {} bytes to {}", n, addr);

        ensure!(n == self.buf.len(), "Failed to send complete message");

        Ok(())
    }

    async fn recv(&mut self) -> anyhow::Result<(Msg<'_, '_>, SocketAddr)> {
        let (n, addr) = self.socket.recv_from(&mut self.recv_buf).await?;
        trace!("Received: {} bytes from {}", n, addr);

        let msg = self.parser.parse(&self.recv_buf[..n])?;
        Ok((msg, addr))
    }

    async fn recv_timeout(
        &mut self,
        timeout: Duration,
    ) -> anyhow::Result<Option<(Msg<'_, '_>, SocketAddr)>> {
        match tokio::time::timeout(timeout, self.recv()).await {
            Ok(x) => x.map(Some),
            Err(_) => Ok(None),
        }
    }
}

struct Transactions {
    pending: HashMap<TxnId, (SocketAddr, Instant)>,
    timeout: Duration,
}

impl Transactions {
    fn new() -> Self {
        Self::with_timeout(Duration::from_secs(5))
    }

    fn with_timeout(timeout: Duration) -> Self {
        Self {
            pending: HashMap::new(),
            timeout,
        }
    }

    fn insert(&mut self, txn_id: TxnId, addr: SocketAddr) {
        self.pending.insert(txn_id, (addr, Instant::now()));
    }

    fn remove(&mut self, txn_id: &TxnId) -> Option<SocketAddr> {
        self.pending.remove(txn_id).map(|(addr, _)| addr)
    }

    fn len(&self) -> usize {
        self.pending.len()
    }

    fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Remove transactions that are timed out or not in Routing table
    /// anymore.
    fn prune(&mut self, table: &mut RoutingTable) {
        let timeout = self.timeout;

        self.pending.retain(|txn_id, (addr, queried)| {
            if Instant::now() - *queried < timeout {
                // Not timed out. Keep it.
                return true;
            }

            if let Some(c) = table.find_contact(addr) {
                c.timed_out();
            }

            trace!("Txn {:?} expired", txn_id);
            false
        });
    }
}
