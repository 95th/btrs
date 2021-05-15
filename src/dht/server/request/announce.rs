use crate::dht::bucket::Bucket;
use crate::dht::id::NodeId;
use crate::dht::msg::recv::Response;
use crate::dht::msg::send::AnnouncePeer;
use crate::dht::server::request::GetPeersRequest;
use crate::dht::server::request::Status;
use crate::dht::server::{PeerSender, RpcMgr};
use crate::dht::table::RoutingTable;
use std::net::SocketAddr;

pub struct AnnounceRequest {
    inner: GetPeersRequest,
}

impl AnnounceRequest {
    pub(super) fn new(
        info_hash: &NodeId,
        own_id: &NodeId,
        tx: PeerSender,
        table: &mut RoutingTable,
    ) -> Self {
        Self {
            inner: GetPeersRequest::new(info_hash, own_id, tx, table),
        }
    }

    pub fn prune(&mut self, table: &mut RoutingTable) {
        self.inner.prune(table);
    }

    pub async fn handle_reply(
        &mut self,
        resp: &Response<'_, '_>,
        addr: &SocketAddr,
        table: &mut RoutingTable,
    ) -> bool {
        self.inner.handle_reply(resp, addr, table).await
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
                    log::debug!("Announced to {}", n.addr);
                }
                Err(e) => log::warn!("Announce failed to {}: {}", n.addr, e),
            }
        }

        true
    }

    pub fn done(self) {
        log::debug!("Done ANNOUNCE")
    }
}