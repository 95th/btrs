use ben::Encode;

use crate::bucket::Bucket;
use crate::id::NodeId;
use crate::msg::recv::Response;
use crate::msg::send::AnnouncePeer;
use crate::server::request::Status;
use crate::server::PeerSender;
use crate::server::RpcMgr;
use crate::table::RoutingTable;
use std::net::SocketAddr;

use super::DhtGetPeers;

pub struct DhtAnnounce {
    inner: DhtGetPeers,
}

impl DhtAnnounce {
    pub fn new(
        info_hash: &NodeId,
        table: &mut RoutingTable,
        sender: PeerSender,
        traversal_id: usize,
    ) -> Self {
        Self {
            inner: DhtGetPeers::new(info_hash, table, sender, traversal_id),
        }
    }

    pub fn handle_response(
        &mut self,
        resp: &Response<'_, '_>,
        addr: &SocketAddr,
        table: &mut RoutingTable,
        rpc: &mut RpcMgr,
        has_id: bool,
    ) {
        log::trace!("Handle ANNOUNCE response");
        self.inner.handle_response(resp, addr, table, rpc, has_id);
    }

    pub fn set_failed(&mut self, id: &NodeId, addr: &SocketAddr) {
        self.inner.set_failed(id, addr);
    }

    pub async fn add_requests(&mut self, rpc: &mut RpcMgr<'_>) -> bool {
        log::trace!("Add ANNOUNCE's GET_PEERS requests");

        let done = self.inner.add_requests(rpc).await;
        if !done {
            return false;
        }

        log::trace!("Finished ANNOUNCE's GET_PEERS. Time to announce");

        let mut announce_count = 0;
        for n in &self.inner.traversal.nodes {
            if announce_count == Bucket::MAX_LEN {
                break;
            }

            if !n.status.contains(Status::ALIVE) {
                continue;
            }

            let txn_id = rpc.new_txn();
            let token = match rpc.tokens.get(&n.addr) {
                Some(t) => t,
                None => {
                    log::warn!("Token not found for {}", n.addr);
                    continue;
                }
            };

            let msg = AnnouncePeer {
                txn_id,
                id: &rpc.own_id,
                info_hash: &self.inner.traversal.target,
                port: 0,
                implied_port: true,
                token,
            };

            msg.encode(&mut rpc.buf);

            match rpc.send(n.addr).await {
                Ok(_) => {
                    log::debug!("Announced to {}", n.addr);
                    announce_count += 1;
                }
                Err(e) => {
                    log::warn!("Failed to announce to {}: {}", n.addr, e);
                }
            }
        }

        if announce_count == 0 {
            log::warn!("Couldn't announce to anyone");
        }

        true
    }

    pub fn done(self) {
        self.inner.done()
    }
}
