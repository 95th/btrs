use crate::contact::{CompactNodes, CompactNodesV6, ContactStatus};
use crate::id::NodeId;
use crate::msg::{AnnouncePeer, FindNode, GetPeers, Msg, MsgKind, TxnId};
use crate::table::RoutingTable;
use anyhow::Context;
use ben::{Encode, Parser};
use std::{
    collections::HashMap,
    net::SocketAddr,
    time::{Duration, Instant},
};
use tokio::net::UdpSocket;

pub struct Server {
    conn: UdpSocket,
    table: RoutingTable,
    parser: Parser,
    txn_id: TxnId,
    buf: Vec<u8>,
}

impl Server {
    pub async fn new(port: u16) -> anyhow::Result<Server> {
        let addr = SocketAddr::from(([0u8; 4], port));
        let conn = UdpSocket::bind(addr).await?;
        let id = NodeId::gen();

        Ok(Server {
            conn,
            table: RoutingTable::new(id),
            parser: Parser::new(),
            txn_id: TxnId(0),
            buf: Vec::with_capacity(1000),
        })
    }

    pub async fn boostrap(&mut self, addrs: &[SocketAddr]) -> anyhow::Result<()> {
        for addr in addrs {
            let txn_id = self.txn_id.next();
            let id = &self.table.own_id;
            let buf = &mut self.buf;
            let parser = &mut self.parser;

            let request = FindNode {
                txn_id,
                id,
                target: id,
            };

            buf.clear();
            request.encode(buf);
            trace!("Sending: {:#?}", parser.parse(buf).unwrap());
            let n = self.conn.send_to(buf, addr).await?;
            trace!("Sent: {} bytes", n);

            buf.resize(1000, 0);
            let (n, rx_addr) = self.conn.recv_from(buf).await?;
            ensure!(rx_addr == *addr, "Address mismatch");
            trace!("Received: {} bytes", n);

            let msg = parser.parse_into::<Msg>(&buf[..n])?;
            trace!("Data: {:#?}", msg);
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

            self.buf.clear();
            msg.encode(&mut self.buf);

            match self.conn.send_to(&self.buf, contact.addr).await {
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
        self.buf.resize(1000, 0);
        let (n, _) = self.conn.recv_from(&mut self.buf[..]).await?;
        let msg = self.parser.parse_into::<Msg>(&self.buf[..n])?;

        info!("message: {:?}", msg);
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
        let txn_id = self.txn_id.next();
        let req = AnnouncePeer {
            id: &self.table.own_id,
            implied_port: true,
            port: 0,
            token: &[0],
            info_hash,
            txn_id,
        };

        self.buf.clear();
        req.encode(&mut self.buf);
        trace!("Sending: {:#?}", self.parser.parse(&self.buf).unwrap());

        let mut closest = Vec::with_capacity(8);
        self.table.find_closest(info_hash, &mut closest);

        let mut pending = vec![];
        for c in &closest {
            match self.conn.send_to(&self.buf, c.addr).await {
                Ok(_) => pending.push(c.addr),
                Err(e) => warn!("{}", e),
            }
        }

        while !pending.is_empty() {
            self.buf.clear();
            let (n, rx_addr) = self.conn.recv_from(&mut self.buf).await?;
            if let Some(i) = pending.iter().position(|a| *a == rx_addr) {
                pending.remove(i);
            }
            let result = self.parser.parse_into::<Msg>(&self.buf[..n]);
            let msg = match result {
                Ok(msg) => msg,
                Err(e) => {
                    trace!("{}", e);
                    continue;
                }
            };

            trace!("{:#?}", msg);
        }

        Ok(())
    }
}

impl RoutingTable {
    fn read_nodes(&mut self, msg: &Msg<'_, '_>) -> anyhow::Result<()> {
        if let MsgKind::Response = msg.kind {
            let dict = msg.body.as_dict().context("Response must be a dict")?;
            let resp = dict.get_dict(b"r").context("Response dict expected")?;

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
