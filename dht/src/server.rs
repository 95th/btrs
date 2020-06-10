use crate::contact::CompactNodes;
use crate::id::NodeId;
use crate::msg::{FindNode, Msg, MsgKind, TxnId};
use crate::table::RoutingTable;
use anyhow::Context;
use ben::{Encode, Parser};
use std::net::SocketAddr;
use tokio::net::UdpSocket;

pub struct Server {
    conn: UdpSocket,
    table: RoutingTable,
    parser: Parser,
    buf: Vec<u8>,
}

impl Server {
    pub async fn new(port: u16) -> anyhow::Result<Server> {
        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        let conn = UdpSocket::bind(addr).await?;
        let id = NodeId::gen();

        Ok(Server {
            conn,
            table: RoutingTable::new(id),
            parser: Parser::new(),
            buf: Vec::with_capacity(1000),
        })
    }

    pub async fn boostrap(&mut self, addrs: &[SocketAddr]) -> anyhow::Result<()> {
        let id = &self.table.own_id;
        let buf = &mut self.buf;
        let parser = &mut self.parser;

        for addr in addrs {
            buf.clear();
            let request = FindNode {
                txn_id: TxnId(11),
                id,
                target: id,
            };
            request.encode(buf);
            trace!("Sending: {:#?}", parser.parse(buf).unwrap());
            let n = self.conn.send_to(buf, addr).await?;
            trace!("Sent: {} bytes", n);

            buf.resize(1000, 0);
            let (n, raddr) = self.conn.recv_from(buf).await?;
            ensure!(raddr == *addr, "Address mismatch");
            trace!("Received: {} bytes", n);

            let msg = Msg::parse(&buf[..n], parser)?;
            trace!("Data: {:#?}", msg);
            ensure!(msg.txn_id.0 == 11, "Transaction ID mismatch");

            if let MsgKind::Response = msg.kind {
                let d = msg.body.as_dict().context("Response must be a dict")?;
                let r = d.get_dict(b"r").context("Response dict expected")?;
                let nodes = r.get_bytes(b"nodes").context("nodes required")?;
                trace!("Nodes.len(): {}", nodes.len());

                for c in CompactNodes::new(nodes)? {
                    trace!("id: {:?}", c);
                }
            }

            break;
        }

        Ok(())
    }
}
