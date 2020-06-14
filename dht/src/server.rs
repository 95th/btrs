use crate::contact::{CompactNodes, CompactNodesV6};
use crate::id::NodeId;
use crate::msg::{AnnouncePeer, FindNode, Msg, MsgKind, TxnId};
use crate::table::RoutingTable;
use anyhow::Context;
use ben::{Encode, Parser};
use std::net::SocketAddr;
use tokio::net::UdpSocket;

pub struct Server {
    conn: UdpSocket,
    table: RoutingTable,
    parser: Parser,
    txn_id: u16,
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
            txn_id: 0,
            buf: Vec::with_capacity(1000),
        })
    }

    pub async fn boostrap(&mut self, addrs: &[SocketAddr]) -> anyhow::Result<()> {
        for addr in addrs {
            let txn_id = self.next_txn_id();
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

            let msg = Msg::parse(&buf[..n], parser)?;
            trace!("Data: {:#?}", msg);
            ensure!(msg.txn_id.0 == 11, "Transaction ID mismatch");

            if let MsgKind::Response = msg.kind {
                let d = msg.body.as_dict().context("Response must be a dict")?;
                let r = d.get_dict(b"r").context("Response dict expected")?;

                let nodes = r.get_bytes(b"nodes").context("nodes required")?;
                for c in CompactNodes::new(nodes)? {
                    self.table.add_contact(&c);
                }

                if let Some(nodes6) = r.get_bytes(b"nodes6") {
                    for c in CompactNodesV6::new(nodes6)? {
                        self.table.add_contact(&c);
                    }
                }
            }
        }

        trace!("{:#?}", self.table);
        Ok(())
    }

    pub async fn announce(&mut self, info_hash: &NodeId) -> anyhow::Result<()> {
        let txn_id = self.next_txn_id();
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

        let closest = self.table.find_closest(info_hash, 8);
        let mut pending = vec![];
        for c in &closest {
            self.conn.send_to(&self.buf, c.addr).await?;
            pending.push(c.addr);
        }

        while !pending.is_empty() {
            self.buf.clear();
            let (n, rx_addr) = self.conn.recv_from(&mut self.buf).await?;
            if let Some(i) = pending.iter().position(|a| *a == rx_addr) {
                pending.remove(i);
            }
            let msg = match Msg::parse(&self.buf[..n], &mut self.parser) {
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

    fn next_txn_id(&mut self) -> TxnId {
        let t = self.txn_id;
        self.txn_id = self.txn_id.wrapping_add(1);
        TxnId(t)
    }
}
