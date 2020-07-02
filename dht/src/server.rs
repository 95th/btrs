use crate::contact::{CompactNodes, CompactNodesV6, ContactStatus};
use crate::id::NodeId;
use crate::msg::{AnnouncePeer, FindNode, GetPeers, Msg, MsgKind, TxnId};
use crate::table::RoutingTable;
use anyhow::Context;
use ben::{Decode, Decoder, Encode, Parser};
use std::{
    collections::HashMap,
    net::SocketAddr,
    time::{Duration, Instant},
};
use tokio::net::UdpSocket;

pub struct Server {
    socket: BufSocket,
    table: RoutingTable,
    txn_id: TxnId,
}

impl Server {
    pub async fn new(port: u16) -> anyhow::Result<Server> {
        let addr = SocketAddr::from(([0u8; 4], port));
        let socket = UdpSocket::bind(addr).await?;
        let id = NodeId::gen();

        Ok(Server {
            socket: BufSocket::new(socket),
            table: RoutingTable::new(id),
            txn_id: TxnId(0),
        })
    }

    pub async fn boostrap(&mut self, addrs: &[SocketAddr]) -> anyhow::Result<()> {
        for addr in addrs {
            let txn_id = self.txn_id.next();
            let id = &self.table.own_id;

            let request = FindNode {
                txn_id,
                id,
                target: id,
            };

            self.socket.send(request, addr).await?;

            let (msg, rx_addr) = self.socket.recv::<Msg>().await?;

            ensure!(rx_addr == *addr, "Address mismatch");
            ensure!(msg.txn_id == txn_id, "Transaction ID mismatch");

            self.table.read_nodes(&msg)?;
        }

        trace!("{:#?}", self.table);
        Ok(())
    }

    pub async fn get_peers(&mut self, info_hash: &NodeId) -> anyhow::Result<()> {
        let mut closest = Vec::with_capacity(8);
        let own_id = self.table.find_closest(info_hash, &mut closest);
        if closest.is_empty() {
            return Ok(());
        }

        let mut txn_id = self.txn_id.next();
        let mut pending = HashMap::new();
        for contact in closest {
            let msg = GetPeers {
                id: own_id,
                info_hash,
                txn_id,
            };

            match self.socket.send(msg, &contact.addr).await {
                Ok(_) => {
                    pending.insert(txn_id, contact.id.clone());
                    contact.status = ContactStatus::QUERIED;
                    txn_id = self.txn_id.next();
                }
                Err(e) => {
                    contact.status = ContactStatus::FAILED;
                    warn!("{}", e);
                }
            }
        }

        let timeout = Instant::now() + Duration::from_secs(5);

        while !pending.is_empty() && Instant::now() < timeout {
            if let Err(e) = self.read_get_peers_response(&mut pending).await {
                warn!("{}", e);
            }
        }

        for (_, id) in pending {
            if let Some(c) = self.table.find_contact(&id) {
                c.status = ContactStatus::FAILED;
                c.timed_out();
            }
        }

        Ok(())
    }

    async fn read_get_peers_response(
        &mut self,
        pending: &mut HashMap<TxnId, NodeId>,
    ) -> anyhow::Result<()> {
        let (msg, _) = self.socket.recv::<Msg>().await?;

        let id = pending
            .remove(&msg.txn_id)
            .context("Response received from unexpected address")?;

        let rx_id = msg.id.context("Node ID is required")?;
        ensure!(rx_id == &id, "Node ID mismatch");
        ensure!(msg.kind == MsgKind::Response, "Expected response");

        self.table.read_nodes(&msg)
    }

    pub async fn announce(&mut self, info_hash: &NodeId) -> anyhow::Result<()> {
        self.get_peers(info_hash).await?;

        let mut closest = Vec::with_capacity(8);
        let own_id = self.table.find_closest(info_hash, &mut closest);

        let mut pending = vec![];
        for c in &closest {
            let txn_id = self.txn_id.next();
            let req = AnnouncePeer {
                id: own_id,
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
            let (_msg, rx_addr) = self.socket.recv::<Msg>().await?;
            if let Some(i) = pending.iter().position(|a| *a == rx_addr) {
                pending.remove(i);
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
