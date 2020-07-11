use crate::bucket::Bucket;
use crate::contact::{CompactNodes, CompactNodesV6, ContactStatus};
use crate::id::NodeId;
use crate::msg::{FindNode, Msg, MsgKind, TxnId};
use crate::table::RoutingTable;
use anyhow::Context as _;
use ben::{Decoder, Encode, Parser};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;

pub struct Server {
    socket: BufSocket,
    table: RoutingTable,
    txn_id: TxnId,
    own_id: NodeId,
    txns: HashMap<TxnId, SocketAddr>,
    bootstrapping: bool,
    bootstrap_nodes: Vec<SocketAddr>,
    next_refresh: Instant,
}

impl Server {
    pub async fn new(port: u16, bootstrap_nodes: Vec<SocketAddr>) -> anyhow::Result<Server> {
        let addr = SocketAddr::from(([0u8; 4], port));
        let socket = UdpSocket::bind(addr).await?;
        let id = NodeId::gen();

        Ok(Server {
            socket: BufSocket::new(socket),
            table: RoutingTable::new(id.clone()),
            txn_id: TxnId(0),
            own_id: id,
            txns: HashMap::new(),
            bootstrapping: true,
            bootstrap_nodes,
            next_refresh: Instant::now(),
        })
    }

    pub async fn run(mut self) {
        loop {
            self.bootstrap().await;

            if self.check_client_request() {
                // TODO: Save DHT state on disk
                break;
            }

            self.recv_response_timeout(Duration::from_millis(100)).await;

            // Self refresh after 15 mins
            const REFRESH_INTERVAL: Duration = Duration::from_secs(15 * 60);

            if Instant::now() >= self.next_refresh {
                self.bootstrapping = true;
                self.next_refresh = Instant::now() + REFRESH_INTERVAL;
            }

            self.check_stale_transactions();
        }
    }

    async fn bootstrap(&mut self) {
        if !self.bootstrapping {
            return;
        }

        let mut min_dist = NodeId::max();
        let own_id = &self.own_id.clone();

        loop {
            let sent = self.find_node(own_id).await;
            if sent == 0 {
                break;
            }
            self.recv_response().await;

            let dist = self.table.min_dist(own_id);
            if dist < min_dist {
                min_dist = dist;
                trace!("Found closer distance: {:?}", min_dist);
            } else {
                trace!("Reached closest distance: {:?}", min_dist);
                break;
            }
        }

        self.bootstrapping = false;
    }

    async fn find_node(&mut self, target: &NodeId) -> usize {
        let mut contacts = vec![];
        self.table
            .find_closest(target, &mut contacts, Bucket::MAX_LEN);

        let mut sent = 0;
        if contacts.is_empty() {
            trace!("No contacts in routing table, use bootstrap nodes");
            for node in &self.bootstrap_nodes {
                let m = FindNode {
                    id: &self.own_id,
                    target,
                    txn_id: self.txn_id.next(),
                };
                match self.socket.send(&m, node).await {
                    Ok(_) => {
                        sent += 1;
                        self.txns.insert(m.txn_id, *node);
                    }
                    Err(e) => warn!("FIND_NODE to bootstrap node {} failed: {}", node, e),
                }
            }
        }

        for c in contacts {
            let m = FindNode {
                id: &self.own_id,
                target,
                txn_id: self.txn_id.next(),
            };
            match self.socket.send(&m, &c.addr).await {
                Ok(_) => {
                    c.status = ContactStatus::QUERIED;
                    c.last_queried = Instant::now();
                    self.txns.insert(m.txn_id, c.addr);
                    sent += 1;
                }
                Err(e) => {
                    warn!("FIND_NODE to {} failed: {}", c.addr, e);
                }
            }
        }
        sent
    }

    fn check_client_request(&mut self) -> bool {
        // TODO
        false
    }

    async fn recv_response(&mut self) {
        match self.socket.recv().await {
            Ok((msg, _addr)) => Self::handle_response(msg, &mut self.table, &mut self.txns),
            Err(e) => warn!("{}", e),
        }
    }

    async fn recv_response_timeout(&mut self, timeout: Duration) {
        match self.socket.recv_timeout(timeout).await {
            Ok(Some((msg, _addr))) => Self::handle_response(msg, &mut self.table, &mut self.txns),
            Ok(None) => {}
            Err(e) => warn!("{}", e),
        }
    }

    fn handle_response(msg: Msg, table: &mut RoutingTable, txns: &mut HashMap<TxnId, SocketAddr>) {
        if let MsgKind::Response = msg.kind {
            let addr = match txns.remove(&msg.txn_id) {
                Some(x) => x,
                None => {
                    warn!(
                        "Message received (txn id: {:?}) from unexpected address",
                        msg.txn_id
                    );
                    return;
                }
            };

            if let Some(c) = table.find_contact(&addr) {
                c.status = ContactStatus::ALIVE | ContactStatus::QUERIED;
                c.clear_timeout();
                c.last_updated = Instant::now();
            }

            table.handle_response(&msg);
            return;
        }

        if let MsgKind::Error = msg.kind {
            warn!("{:?}", msg);
            return;
        }

        table.handle_query(&msg);
    }

    fn check_stale_transactions(&mut self) {
        const TIMEOUT: Duration = Duration::from_secs(5);

        let txns = std::mem::replace(&mut self.txns, HashMap::new());
        for (txn_id, addr) in txns {
            if let Some(c) = self.table.find_contact(&addr) {
                if Instant::now() - c.last_queried < TIMEOUT {
                    // Not timed out, keep it
                    self.txns.insert(txn_id, addr);
                } else {
                    c.timed_out();
                }
            }
        }
    }
}

impl RoutingTable {
    fn handle_query(&mut self, msg: &Msg) {
        debug!("Got query request: {:#?}", msg);
    }

    fn handle_response(&mut self, msg: &Msg) {
        match msg.kind {
            MsgKind::Response => {
                if let Err(e) = self.read_nodes(msg) {
                    warn!("{}", e);
                }
            }
            _ => {}
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
        }
        Ok(())
    }
}

struct BufSocket {
    socket: UdpSocket,
    buf: Vec<u8>,
    recv_buf: Box<[u8]>,
    parser: Parser,
}

impl BufSocket {
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
        trace!("Sent: {} bytes", n);

        ensure!(n == self.buf.len(), "Failed to send complete message");

        Ok(())
    }

    async fn recv<'a>(&'a mut self) -> anyhow::Result<(Msg<'a, 'a>, SocketAddr)> {
        let (n, addr) = self.socket.recv_from(&mut self.recv_buf).await?;
        trace!("Received: {} bytes", n);

        let msg = self.parser.parse(&self.recv_buf[..n])?;
        Ok((msg, addr))
    }

    async fn recv_timeout<'a>(
        &'a mut self,
        timeout: Duration,
    ) -> anyhow::Result<Option<(Msg<'a, 'a>, SocketAddr)>> {
        match tokio::time::timeout(timeout, self.recv()).await {
            Ok(x) => x.map(Some),
            Err(_) => Ok(None),
        }
    }
}
