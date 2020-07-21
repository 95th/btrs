use crate::bucket::Bucket;
use crate::id::NodeId;
use crate::msg::recv::Response;
use crate::msg::send::AnnouncePeer;
use crate::server::traversal::GetPeersTraversal;
use crate::server::traversal::Status;
use crate::server::{PeerSender, RpcMgr};
use crate::table::RoutingTable;
use std::net::SocketAddr;

pub struct AnnounceTraversal {
    inner: GetPeersTraversal,
}

impl AnnounceTraversal {
    pub fn new(info_hash: &NodeId, own_id: &NodeId, tx: PeerSender) -> Self {
        Self {
            inner: GetPeersTraversal::new(info_hash, own_id, tx),
        }
    }

    pub async fn start(&mut self, table: &mut RoutingTable, rpc: &mut RpcMgr) {
        self.inner.start(table, rpc).await
    }

    pub fn prune(&mut self, table: &mut RoutingTable) {
        self.inner.prune(table);
    }

    pub fn handle_reply(
        &mut self,
        resp: &Response,
        addr: &SocketAddr,
        table: &mut RoutingTable,
    ) -> bool {
        self.inner.handle_reply(resp, addr, table)
    }

    pub async fn invoke(&mut self, rpc: &mut RpcMgr) -> bool {
        if !self.inner.invoke(rpc).await {
            return false;
        }

        let mut announce_count = 0;
        for n in &self.inner.nodes {
            if announce_count == Bucket::MAX_LEN {
                break;
            }

            if !n.status.contains(Status::ALIVE) {
                continue;
            }

            let token = match self.inner.tokens.get(&n.addr) {
                Some(t) => t,
                None => continue,
            };

            let msg = AnnouncePeer {
                id: &self.inner.own_id,
                info_hash: &self.inner.info_hash,
                port: 0,
                implied_port: true,
                txn_id: rpc.next_id(),
                token,
            };

            match rpc.send(&msg, &n.addr).await {
                Ok(_) => {
                    announce_count += 1;
                    debug!("Announced to {}", n.addr);
                }
                Err(e) => warn!("Announce failed to {}: {}", n.addr, e),
            }
        }

        true
    }

    pub fn done(self) {
        self.inner.done();
    }
}
