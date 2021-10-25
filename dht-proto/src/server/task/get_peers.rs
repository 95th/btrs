use crate::id::NodeId;
use crate::msg::recv::Response;
use crate::msg::send::GetPeers;
use crate::server::rpc::Event;
use crate::server::RpcManager;
use crate::table::RoutingTable;
use ben::{Decoder, Encode};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::time::Instant;

use super::base::BaseTask;
use super::{Task, TaskId};

pub struct GetPeersTask {
    pub base: BaseTask,
    peers: HashSet<SocketAddr>,
}

impl GetPeersTask {
    pub fn new(info_hash: &NodeId, table: &RoutingTable, task_id: TaskId) -> Self {
        Self {
            base: BaseTask::new(info_hash, table, task_id),
            peers: HashSet::new(),
        }
    }
}

impl Task for GetPeersTask {
    fn id(&self) -> TaskId {
        self.base.task_id
    }

    fn handle_response(
        &mut self,
        resp: &Response<'_>,
        addr: &SocketAddr,
        table: &mut RoutingTable,
        rpc: &mut RpcManager,
        has_id: bool,
        now: Instant,
    ) {
        log::trace!("Handle GET_PEERS response");
        self.base.handle_response(resp, addr, table, has_id, now);

        if let Some(token) = resp.body.get_bytes("token") {
            rpc.tokens.insert(*addr, token.to_vec());
        }

        if let Some(peers) = resp.body.get_list("values") {
            let peers = peers.into_iter().flat_map(decode_peer);
            self.peers.extend(peers);
        }
    }

    fn set_failed(&mut self, id: &NodeId, addr: &SocketAddr) {
        self.base.set_failed(id, addr);
    }

    fn add_requests(&mut self, rpc: &mut RpcManager, now: Instant) -> bool {
        log::trace!("Add GET_PEERS requests");

        let info_hash = self.base.target;
        self.base.add_requests(rpc, now, |buf, rpc| {
            let msg = GetPeers {
                txn_id: rpc.new_txn(),
                id: &rpc.own_id,
                info_hash: &info_hash,
            };

            log::trace!("Send {:?}", msg);

            msg.encode(buf);
            msg.txn_id
        })
    }

    fn done(&mut self, rpc: &mut RpcManager) {
        log::info!("Found {} peers", self.peers.len());
        rpc.add_event(Event::FoundPeers {
            task_id: self.base.task_id,
            peers: std::mem::take(&mut self.peers),
        });
    }
}

fn decode_peer(d: Decoder) -> Option<SocketAddr> {
    if let Some(b) = d.as_bytes() {
        if b.len() == 6 {
            unsafe {
                let p = b.as_ptr();
                let ip = *p.cast::<[u8; 4]>();
                let port = u16::from_be_bytes(*p.add(4).cast());
                return Some((ip, port).into());
            }
        } else {
            log::warn!("Incorrect Peer length. Expected: 6, Actual: {}", b.len());
        }
    } else {
        log::warn!("Unexpected Peer format: {:?}", d);
    }

    None
}
