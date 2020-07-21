use crate::bucket::Bucket;
use crate::contact::{CompactNodes, CompactNodesV6, ContactRef, ContactStatus};
use crate::id::NodeId;
use crate::msg::recv::{Msg, Query, Response};
use crate::msg::send::{AnnouncePeer, FindNode, GetPeers};
use crate::msg::TxnId;
use crate::table::RoutingTable;
use ben::{Decoder, Encode, Parser};
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::oneshot;

type PeerSender = oneshot::Sender<Vec<SocketAddr>>;

pub struct Server {
    rpc: RpcMgr,
    table: RoutingTable,
    own_id: NodeId,
    txns: Transactions,
    router_nodes: Vec<SocketAddr>,
    next_refresh: Instant,
    client_rx: Receiver<ClientRequest>,
    client_tx: Sender<ClientRequest>,
    running: Vec<Traversal>,
}

#[derive(Clone)]
pub struct Client {
    pub tx: Sender<ClientRequest>,
}

#[derive(Debug)]
pub enum ClientRequest {
    GetPeers(NodeId, PeerSender),
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
            own_id: id,
            txns: Transactions::new(),
            router_nodes,
            next_refresh: Instant::now(),
            client_tx,
            client_rx,
            running: vec![],
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

            // Housekeep running requests
            self.check_running().await;

            // Clear stale transactions
            self.txns.prune(&mut self.table);
        }
    }

    async fn check_running(&mut self) {
        let mut i = 0;
        while let Some(t) = self.running.get_mut(i) {
            t.prune(&mut self.table);

            if t.invoke(&mut self.rpc).await {
                let t = self.running.swap_remove(i);
                t.done();
            } else {
                i += 1;
            }
        }
    }

    async fn refresh(&mut self, target: &NodeId) {
        let mut nodes = VecDeque::new();
        let mut min_dist = NodeId::max();
        let zero_id = NodeId::new();

        if !self.table.is_empty() {
            let mut closest = Vec::with_capacity(Bucket::MAX_LEN);
            self.table
                .find_closest(target, &mut closest, Bucket::MAX_LEN);

            for c in closest {
                nodes.push_front((*c.id, c.addr));
                let dist = target ^ c.id;
                min_dist = min_dist.min(dist);
            }
        }

        if nodes.len() < 3 {
            for addr in &self.router_nodes {
                nodes.push_back((zero_id, *addr));
            }
        }

        debug!("Start refresh with {} nodes", nodes.len());

        let max_outstanding = 3;

        loop {
            while self.txns.len() < max_outstanding {
                if let Some((id, addr)) = nodes.pop_front() {
                    self.find_node(target, &id, &addr).await;
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
                    nodes.push_front((*c.id, c.addr));
                    min_dist = dist;
                }
            }
        }
    }

    pub async fn get_peers(&mut self, info_hash: &NodeId, tx: PeerSender) {
        let mut gp = Box::new(GetPeersTraversal::new(info_hash, &self.own_id, tx));
        gp.start(&mut self.table, &mut self.rpc).await;
        self.running.push(Traversal::GetPeers(gp));
    }

    async fn find_node(&mut self, target: &NodeId, id: &NodeId, addr: &SocketAddr) -> bool {
        debug!("Send FIND_NODE request to {}", addr);

        let m = FindNode {
            id: &self.own_id,
            target,
            txn_id: self.rpc.next_id(),
        };
        match self.rpc.send(&m, addr).await {
            Ok(_) => {
                self.txns.insert(m.txn_id, id);
                true
            }
            Err(e) => {
                warn!("FIND_NODE to {} failed: {}", addr, e);
                false
            }
        }
    }

    async fn check_client_request(&mut self) -> bool {
        match self.client_rx.try_recv() {
            Ok(ClientRequest::GetPeers(info_hash, tx)) => {
                self.get_peers(&info_hash, tx).await;
                false
            }
            Ok(ClientRequest::Shutdown) => true,
            Err(_) => false,
        }
    }

    pub async fn announce(&mut self, info_hash: &NodeId) {
        let mut closest = Vec::with_capacity(Bucket::MAX_LEN);
        self.table
            .find_closest(&info_hash, &mut closest, Bucket::MAX_LEN);

        for c in closest {
            debug!("Send ANNOUNCE_PEER request to {}", c.addr);

            let token = match self.table.tokens.get(&c.id) {
                Some(t) => t,
                None => {
                    debug!("Token not found for {:?}", c.id);
                    continue;
                }
            };

            let m = AnnouncePeer {
                id: &self.own_id,
                info_hash,
                txn_id: self.rpc.next_id(),
                implied_port: true,
                port: 0,
                token,
            };

            match self.rpc.send(&m, &c.addr).await {
                Ok(_) => self.txns.insert(m.txn_id, c.id),
                Err(e) => warn!("ANNOUNCE_PEER to {} failed: {}", c.addr, e),
            }
        }
    }

    async fn recv_response(&mut self, timeout: Duration) {
        let (msg, _addr) = match self.rpc.recv_timeout(timeout).await {
            Ok(Some(x)) => x,
            Ok(None) => return,
            Err(e) => {
                warn!("{}", e);
                return;
            }
        };

        match msg {
            Msg::Response(resp) => {
                for t in &mut self.running {
                    if t.handle_reply(&resp, &mut self.table) {
                        break;
                    }
                }
            }
            _ => self.table.handle_msg(msg, &mut self.txns),
        }
    }
}

impl RoutingTable {
    fn handle_msg(&mut self, msg: Msg, txns: &mut Transactions) {
        match msg {
            Msg::Response(resp) => {
                match txns.remove(&resp.txn_id) {
                    Some(request) => {
                        if request.has_id {
                            if let Some(c) = self.find_contact(&request.id) {
                                c.status = ContactStatus::ALIVE | ContactStatus::QUERIED;
                                c.clear_timeout();
                                c.last_updated = Instant::now();
                            }
                        }
                    }
                    None => {
                        warn!(
                            "Message received (txn id: {:?}) from unexpected address",
                            resp.txn_id
                        );
                        return;
                    }
                };

                self.handle_response(&resp);
            }
            Msg::Error(err) => {
                warn!("{:?} failed: {:#?}", err.txn_id, err.list);
            }
            Msg::Query(query) => self.handle_query(&query),
        }
    }

    fn handle_query(&mut self, query: &Query) {
        debug!("Got query request: {:#?}", query);
    }

    fn handle_response(&mut self, response: &Response) {
        if let Err(e) = self.read_nodes(response) {
            warn!("{}", e);
        }
    }

    fn read_nodes(&mut self, response: &Response) -> anyhow::Result<()> {
        self.read_nodes_with(response, |_| {})
    }

    fn read_nodes_with<F>(&mut self, response: &Response, mut f: F) -> anyhow::Result<()>
    where
        F: FnMut(&ContactRef),
    {
        if let Some(nodes) = response.body.get_bytes("nodes") {
            for c in CompactNodes::new(nodes)? {
                self.add_contact(&c);
                f(&c);
            }
        }

        if let Some(nodes6) = response.body.get_bytes("nodes6") {
            for c in CompactNodesV6::new(nodes6)? {
                self.add_contact(&c);
                f(&c);
            }
        }

        if let Some(token) = response.body.get_bytes("token") {
            self.tokens.insert(*response.id, token.to_vec());
        }

        Ok(())
    }
}

struct RpcMgr {
    socket: UdpSocket,
    buf: Vec<u8>,
    recv_buf: Box<[u8]>,
    parser: Parser,
    txn_id: TxnId,
}

impl RpcMgr {
    fn new(socket: UdpSocket) -> Self {
        Self {
            socket,
            buf: Vec::new(),
            recv_buf: vec![0; 2048].into_boxed_slice(),
            parser: Parser::new(),
            txn_id: TxnId(0),
        }
    }

    fn next_id(&mut self) -> TxnId {
        self.txn_id.next_id()
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

pub struct Request {
    id: NodeId,
    sent: Instant,
    has_id: bool,
}

impl Request {
    fn new(id: &NodeId) -> Self {
        Self {
            id: if id.is_zero() { NodeId::gen() } else { *id },
            sent: Instant::now(),
            has_id: !id.is_zero(),
        }
    }
}

struct Transactions {
    pending: HashMap<TxnId, Request>,
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

    fn insert(&mut self, txn_id: TxnId, id: &NodeId) {
        self.pending.insert(txn_id, Request::new(id));
    }

    fn remove(&mut self, txn_id: &TxnId) -> Option<Request> {
        self.pending.remove(txn_id)
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
        self.prune_with(table, |_| {});
    }

    /// Remove transactions that are timed out or not in Routing table
    /// anymore.
    fn prune_with<F>(&mut self, table: &mut RoutingTable, mut f: F)
    where
        F: FnMut(&NodeId),
    {
        let timeout = self.timeout;

        self.pending.retain(|txn_id, request| {
            if Instant::now() - request.sent < timeout {
                // Not timed out. Keep it.
                return true;
            }

            if request.has_id {
                if let Some(c) = table.find_contact(&request.id) {
                    c.timed_out();
                }
            }

            f(&request.id);

            trace!("Txn {:?} expired", txn_id);
            false
        });
    }
}

pub struct TraversalNode {
    id: NodeId,
    addr: SocketAddr,
    status: Status,
}

impl TraversalNode {
    fn new(c: &ContactRef) -> Self {
        Self {
            id: *c.id,
            addr: c.addr,
            status: Status::INITIAL,
        }
    }
}

bitflags! {
    pub struct Status: u8 {
        const INITIAL   = 0x01;
        const ALIVE     = 0x02;
        const FAILED    = 0x04;
        const NO_ID     = 0x08;
        const QUERIED   = 0x10;
    }
}

pub enum Traversal {
    GetPeers(Box<GetPeersTraversal>),
}

impl Traversal {
    fn prune(&mut self, table: &mut RoutingTable) {
        match self {
            Self::GetPeers(gp) => gp.prune(table),
        }
    }

    fn handle_reply(&mut self, resp: &Response, table: &mut RoutingTable) -> bool {
        match self {
            Self::GetPeers(gp) => gp.handle_reply(resp, table),
        }
    }

    async fn invoke(&mut self, rpc: &mut RpcMgr) -> bool {
        match self {
            Self::GetPeers(gp) => gp.invoke(rpc).await,
        }
    }

    fn done(self) {
        match self {
            Self::GetPeers(gp) => gp.done(),
        }
    }
}

pub struct GetPeersTraversal {
    info_hash: NodeId,
    own_id: NodeId,
    nodes: Vec<TraversalNode>,
    peers: Vec<SocketAddr>,
    txns: Transactions,
    tx: PeerSender,
    branch_factor: u8,
}

impl GetPeersTraversal {
    pub fn new(info_hash: &NodeId, own_id: &NodeId, tx: PeerSender) -> Self {
        Self {
            info_hash: *info_hash,
            own_id: *own_id,
            nodes: vec![],
            peers: vec![],
            txns: Transactions::new(),
            tx,
            branch_factor: 3,
        }
    }
}

impl GetPeersTraversal {
    async fn start(&mut self, table: &mut RoutingTable, rpc: &mut RpcMgr) {
        let mut closest = Vec::with_capacity(Bucket::MAX_LEN);
        table.find_closest(&self.info_hash, &mut closest, Bucket::MAX_LEN);
        for c in closest {
            self.nodes.push(TraversalNode::new(&c));
        }

        if self.nodes.len() < 3 {
            for node in &table.router_nodes {
                self.nodes.push(TraversalNode {
                    id: NodeId::gen(),
                    addr: *node,
                    status: Status::INITIAL | Status::NO_ID,
                });
            }
        }

        self.invoke(rpc).await;
    }

    fn prune(&mut self, table: &mut RoutingTable) {
        let nodes = &mut self.nodes;
        self.txns.prune_with(table, |id| {
            if let Some(node) = nodes.iter_mut().find(|node| &node.id == id) {
                node.status.insert(Status::FAILED);
            }
        })
    }

    fn handle_reply(&mut self, resp: &Response, table: &mut RoutingTable) -> bool {
        if let Some(req) = self.txns.remove(&resp.txn_id) {
            if req.has_id {
                table.heard_from(&req.id);
            }
        } else {
            return false;
        }

        if let Some(node) = self.nodes.iter_mut().find(|node| &node.id == resp.id) {
            node.status.insert(Status::ALIVE);
        } else {
            debug_assert!(false, "Shouldn't be here");
            return false;
        }

        let result = table.read_nodes_with(resp, |c| {
            if !self.nodes.iter().any(|n| &n.id == c.id) {
                self.nodes.push(TraversalNode::new(c));
            }
        });

        if let Err(e) = result {
            warn!("{}", e);
        }

        let target = &self.info_hash;
        self.nodes.sort_by_key(|n| n.id ^ target);
        self.nodes.truncate(100);

        true
    }

    async fn invoke(&mut self, rpc: &mut RpcMgr) -> bool {
        let mut outstanding = 0;
        let mut alive = 0;

        for n in &mut self.nodes {
            if alive == Bucket::MAX_LEN {
                break;
            }

            if outstanding == self.branch_factor {
                break;
            }

            if n.status.contains(Status::ALIVE) {
                alive += 1;
                continue;
            }

            if n.status.contains(Status::QUERIED) {
                if !n.status.contains(Status::FAILED) {
                    outstanding += 1;
                }
                continue;
            };

            let msg = GetPeers {
                info_hash: &self.info_hash,
                id: &self.own_id,
                txn_id: rpc.next_id(),
            };

            match rpc.send(&msg, &n.addr).await {
                Ok(_) => {
                    n.status.insert(Status::QUERIED);
                    self.txns.insert(msg.txn_id, &n.id);
                    outstanding += 1;
                }
                Err(e) => {
                    warn!("{}", e);
                    n.status.insert(Status::FAILED);
                }
            }
        }

        outstanding == 0 && alive == Bucket::MAX_LEN
    }

    fn done(self) {
        match self.tx.send(self.peers) {
            Ok(_) => debug!("Replied to GET_PEERS client request"),
            Err(_) => warn!("Reply to GET_PEERS client request failed"),
        }
    }
}
