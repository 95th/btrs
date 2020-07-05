use crate::contact::{CompactNodes, CompactNodesV6, ContactStatus};
use crate::id::NodeId;
use crate::msg::{AnnouncePeer, FindNode, GetPeers, Msg, MsgKind, TxnId};
use crate::table::RoutingTable;
use anyhow::Context;
use ben::{Decode, Decoder, Encode, Parser};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;

pub struct Server {
    socket: BufSocket,
    table: RoutingTable,
    txn_id: TxnId,
    own_id: NodeId,
}

impl Server {
    pub async fn new(port: u16) -> anyhow::Result<Server> {
        let addr = SocketAddr::from(([0u8; 4], port));
        let socket = UdpSocket::bind(addr).await?;
        let id = NodeId::gen();

        Ok(Server {
            socket: BufSocket::new(socket),
            table: RoutingTable::new(id.clone()),
            txn_id: TxnId(0),
            own_id: id,
        })
    }

    pub async fn boostrap(&mut self, addrs: &[SocketAddr]) -> anyhow::Result<()> {
        trace!("Bootstrapping");

        let id = &self.own_id.clone();
        self.find_node(id, addrs).await?;

        let mut distance = NodeId::max();
        loop {
            debug!("distance now: {:?}", distance);
            let closest = self.table.find_closest(id, 8);
            if let Some(closest) = closest.first() {
                let min_dist = &closest.id ^ id;
                if min_dist >= distance {
                    debug!("Minimum distance reached");
                    break;
                }
                debug!("Closer distance found: {:?}", min_dist);
                distance = min_dist;
            } else {
                debug!("No nodes to begin with");
                break;
            }

            let addrs: Vec<_> = closest.iter().map(|c| c.addr).collect();
            self.find_node(id, &addrs).await?;
        }

        trace!("{:#?}", self.table);
        Ok(())
    }

    pub async fn find_node(&mut self, target: &NodeId, addrs: &[SocketAddr]) -> anyhow::Result<()> {
        debug!("Find nodes for target: {:?}", target);

        let mut pending = HashMap::new();
        let mut txn_id = self.txn_id.next();
        for addr in addrs {
            let request = FindNode {
                txn_id,
                id: &self.table.own_id,
                target,
            };

            match self.socket.send(request, addr).await {
                Ok(_) => {
                    pending.insert(txn_id, addr);
                    txn_id = self.txn_id.next();
                }
                Err(e) => {
                    warn!("{}", e);
                    continue;
                }
            }
        }

        debug!("Sent {} FIND_NODE messages", pending.len());

        let f = async {
            while !pending.is_empty() {
                if let Err(e) = self.read_find_node_response(&mut pending).await {
                    warn!("{}", e);
                }
            }
        };

        let timeout = Instant::now() + Duration::from_secs(5);
        if let Err(e) = tokio::time::timeout_at(timeout.into(), f).await {
            warn!("Timed out: {}", e);
        }

        for (_, addr) in pending {
            if let Some(c) = self.table.find_contact(addr) {
                c.status = ContactStatus::FAILED;
                c.timed_out();
            }
        }

        Ok(())
    }

    async fn read_find_node_response(
        &mut self,
        pending: &mut HashMap<TxnId, &SocketAddr>,
    ) -> anyhow::Result<()> {
        let (msg, rx_addr) = self.socket.recv::<Msg>().await?;

        let addr = pending
            .remove(&msg.txn_id)
            .context("Response received from unexpected address")?;

        ensure!(rx_addr == *addr, "Address mismatch");
        ensure!(msg.kind == MsgKind::Response, "Expected response");

        self.table.read_nodes(&msg)
    }

    pub async fn get_peers(&mut self, info_hash: &NodeId) -> anyhow::Result<()> {
        debug!("Get peers for infohash: {:?}", info_hash);

        let closest = self.table.find_closest(info_hash, 8);
        if closest.is_empty() {
            return Ok(());
        }

        let mut txn_id = self.txn_id.next();
        let mut pending = HashMap::new();
        for contact in closest {
            let msg = GetPeers {
                id: &self.own_id,
                info_hash,
                txn_id,
            };

            match self.socket.send(msg, &contact.addr).await {
                Ok(_) => {
                    pending.insert(txn_id, contact.addr);
                    contact.status = ContactStatus::QUERIED;
                    txn_id = self.txn_id.next();
                }
                Err(e) => {
                    contact.status = ContactStatus::FAILED;
                    warn!("{}", e);
                }
            }
        }

        debug!("Sent {} GET_PEERS messages", pending.len());

        let f = async {
            while !pending.is_empty() {
                if let Err(e) = self.read_get_peers_response(&mut pending).await {
                    warn!("{}", e);
                }
            }
        };

        let timeout = Instant::now() + Duration::from_secs(5);
        if let Err(e) = tokio::time::timeout_at(timeout.into(), f).await {
            warn!("Timed out: {}", e);
        }

        for (_, addr) in pending {
            if let Some(c) = self.table.find_contact(&addr) {
                c.status = ContactStatus::FAILED;
                c.timed_out();
            }
        }

        Ok(())
    }

    async fn read_get_peers_response(
        &mut self,
        pending: &mut HashMap<TxnId, SocketAddr>,
    ) -> anyhow::Result<()> {
        let (msg, rx_addr) = self.socket.recv::<Msg>().await?;

        let addr = pending
            .remove(&msg.txn_id)
            .context("Response received from unexpected address")?;

        ensure!(rx_addr == addr, "Address mismatch");
        ensure!(msg.kind == MsgKind::Response, "Expected response");

        self.table.read_nodes(&msg)
    }

    pub async fn announce(&mut self, info_hash: &NodeId) -> anyhow::Result<()> {
        self.get_peers(info_hash).await?;

        let closest = self.table.find_closest(info_hash, 8);

        let mut pending = vec![];
        for c in &closest {
            let txn_id = self.txn_id.next();
            let req = AnnouncePeer {
                id: &self.own_id,
                implied_port: true,
                port: 0,
                token: &[0],
                info_hash,
                txn_id,
            };

            match self.socket.send(&req, &c.addr).await {
                Ok(_) => pending.push(c.addr),
                Err(e) => warn!("{}", e),
            }
        }

        while !pending.is_empty() {
            match self.socket.recv::<Msg>().await {
                Ok((_msg, rx_addr)) => {
                    if let Some(i) = pending.iter().position(|a| *a == rx_addr) {
                        pending.remove(i);
                    }
                }
                Err(e) => warn!("{}", e),
            }
        }

        Ok(())
    }
}

impl RoutingTable {
    fn read_nodes(&mut self, msg: &Msg<'_, '_>) -> anyhow::Result<()> {
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
    parser: Parser,
}

impl BufSocket {
    fn new(socket: UdpSocket) -> Self {
        Self {
            socket,
            buf: Vec::new(),
            parser: Parser::new(),
        }
    }

    async fn send<E: Encode>(&mut self, msg: E, addr: &SocketAddr) -> anyhow::Result<()> {
        self.buf.clear();
        msg.encode(&mut self.buf);

        trace!(
            "Sending: {:#?}",
            self.parser.parse::<Decoder>(&self.buf).unwrap()
        );

        let n = self.socket.send_to(&self.buf, addr).await?;
        trace!("Sent: {} bytes", n);

        ensure!(n == self.buf.len(), "Failed to send complete message");

        Ok(())
    }

    async fn recv<'a, D>(&'a mut self) -> anyhow::Result<(D, SocketAddr)>
    where
        D: Decode<'a, 'a> + std::fmt::Debug,
    {
        self.buf.resize(1000, 0);

        let (n, addr) = self.socket.recv_from(&mut self.buf).await?;
        trace!("Received: {} bytes", n);

        let msg = self.parser.parse(&self.buf[..n])?;
        trace!("message: {:?}", msg);

        Ok((msg, addr))
    }
}
