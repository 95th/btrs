use crate::contact::CompactNodes;
use crate::id::NodeId;
use crate::msg::{FindNode, IncomingMsg, MsgKind, TxnId};
use anyhow::Context;
use ben::{Encode, Parser};
use tokio::net::{lookup_host, UdpSocket};

pub struct Server {
    id: NodeId,
    conn: UdpSocket,
}

impl Server {
    pub async fn boostrap(addr: &str) -> anyhow::Result<Server> {
        let addr = lookup_host(addr)
            .await?
            .next()
            .with_context(|| format!("Unable to resolve host: {}", addr))?;
        trace!("Address resolved to {}", addr);

        let mut conn = UdpSocket::bind("0.0.0.0:6881").await?;

        let id = NodeId::gen();
        let mut buf = Vec::with_capacity(1000);
        let mut parser = Parser::new();
        loop {
            buf.clear();
            let request = FindNode {
                txn_id: TxnId(11),
                id: &id,
                target: &id,
            };
            request.encode(&mut buf);
            trace!("Sending: {:#?}", parser.parse(&buf).unwrap());
            let n = conn.send_to(&buf, addr).await?;
            trace!("Sent: {} bytes", n);

            buf.resize(1000, 0);
            let (n, raddr) = conn.recv_from(&mut buf).await?;
            ensure!(raddr == addr, "Address mismatch");
            trace!("Received: {} bytes", n);

            let msg = IncomingMsg::parse(&buf[..n], &mut parser)?;
            trace!("Data: {:#?}", msg);
            ensure!(msg.txn_id.0 == 11, "Transaction ID mismatch");

            if let MsgKind::Response = msg.kind {
                let d = msg.body.as_dict().context("Response must be a dict")?;
                let r = d.get_dict(b"r").context("Response dict expected")?;
                let nodes = r.get_bytes(b"nodes").context("nodes required")?;
                trace!("Nodes.len(): {}", nodes.len());

                for (id, addr, port) in CompactNodes::new(nodes)? {
                    trace!("id: {:?}", id);
                    trace!("addr: {}, port: {}", addr, port);
                }
            }

            break;
        }

        Ok(Server { id, conn })
    }
}
