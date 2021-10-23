use ben::{Encode, Encoder};

use crate::bucket::Bucket;
use crate::id::NodeId;
use crate::msg::recv::Response;
use crate::msg::send::AnnouncePeer;
use crate::server::task::Status;
use crate::server::RpcManager;
use crate::table::RoutingTable;
use std::net::SocketAddr;

use super::{GetPeersTask, Task, TaskId};

pub struct AnnounceTask {
    get_peers: GetPeersTask,
}

impl AnnounceTask {
    pub fn new(info_hash: &NodeId, table: &mut RoutingTable, task_id: TaskId) -> Self {
        Self {
            get_peers: GetPeersTask::new(info_hash, table, task_id),
        }
    }
}

impl Task for AnnounceTask {
    fn id(&self) -> TaskId {
        self.get_peers.id()
    }

    fn handle_response(
        &mut self,
        resp: &Response<'_>,
        addr: &SocketAddr,
        table: &mut RoutingTable,
        rpc: &mut RpcManager,
        has_id: bool,
    ) {
        log::trace!("Handle ANNOUNCE response");
        self.get_peers
            .handle_response(resp, addr, table, rpc, has_id);
    }

    fn set_failed(&mut self, id: &NodeId, addr: &SocketAddr) {
        self.get_peers.set_failed(id, addr);
    }

    fn add_requests(&mut self, rpc: &mut RpcManager) -> bool {
        log::trace!("Add ANNOUNCE's GET_PEERS requests");

        let done = self.get_peers.add_requests(rpc);
        if !done {
            return false;
        }

        log::trace!("Finished ANNOUNCE's GET_PEERS. Time to announce");

        let mut announce_count = 0;
        for n in &self.get_peers.base.nodes {
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

            let mut buf = Vec::new();
            let msg = AnnouncePeer {
                txn_id,
                id: &rpc.own_id,
                info_hash: &self.get_peers.base.target,
                port: 0,
                implied_port: true,
                token,
            };

            msg.encode(Encoder::new(&mut buf));

            rpc.transmit(self.id(), n.id, buf, n.addr);
            log::debug!("Announced to {}", n.addr);
            announce_count += 1;
        }

        if announce_count == 0 {
            log::warn!("Couldn't announce to anyone");
        }

        true
    }

    fn done(&mut self, rpc: &mut RpcManager) {
        self.get_peers.done(rpc)
    }
}
