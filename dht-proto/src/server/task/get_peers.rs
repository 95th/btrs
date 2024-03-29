use crate::id::NodeId;
use crate::msg::recv::Response;
use crate::msg::send::GetPeers;
use crate::server::rpc::Event;
use crate::server::RpcManager;
use crate::table::RoutingTable;
use ben::{Encode, Entry};
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
    pub fn new(info_hash: NodeId, table: &RoutingTable, task_id: TaskId) -> Self {
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

    #[instrument(skip_all, fields(task = ?self.id()))]
    fn handle_response(
        &mut self,
        resp: &Response<'_>,
        addr: SocketAddr,
        table: &mut RoutingTable,
        rpc: &mut RpcManager,
        has_id: bool,
        now: Instant,
    ) {
        trace!("Handle GET_PEERS response");
        self.base.handle_response(resp, addr, table, has_id, now);

        if let Some(token) = resp.body.get_bytes("token") {
            rpc.tokens.insert(addr, token.to_vec());
        }

        if let Some(peers) = resp.body.get_list("values") {
            let peers = peers.into_iter().flat_map(decode_peer);
            self.peers.extend(peers);
        }

        if let Some(peers) = resp.body.get_list("values6") {
            let peers = peers.into_iter().flat_map(decode_peer);
            self.peers.extend(peers);
        }
    }

    fn set_failed(&mut self, id: NodeId, addr: SocketAddr) {
        self.base.set_failed(id, addr);
    }

    #[instrument(skip_all, fields(task = ?self.id()))]
    fn add_requests(&mut self, rpc: &mut RpcManager, now: Instant) -> bool {
        trace!("Add GET_PEERS requests");

        let info_hash = self.base.target;
        self.base.add_requests(rpc, now, |buf, rpc| {
            let msg = GetPeers {
                txn_id: rpc.new_txn(),
                id: rpc.own_id,
                info_hash,
            };

            trace!("Send {:?}", msg);
            msg.encode(buf);
            msg.txn_id
        })
    }

    fn done(&mut self, rpc: &mut RpcManager) {
        info!("Found {} peers", self.peers.len());
        rpc.add_event(Event::FoundPeers {
            peers: std::mem::take(&mut self.peers),
        });
    }
}

fn decode_peer(d: Entry) -> Option<SocketAddr> {
    if let Some(b) = d.as_bytes() {
        if b.len() == 6 {
            let ptr = b.as_ptr().cast::<([u8; 4], [u8; 2])>();
            let (ip, port) = unsafe { *ptr };
            return Some((ip, u16::from_be_bytes(port)).into());
        } else if b.len() == 18 {
            let ptr = b.as_ptr().cast::<([u8; 16], [u8; 2])>();
            let (ip, port) = unsafe { *ptr };
            return Some((ip, u16::from_be_bytes(port)).into());
        } else {
            warn!("Incorrect Peer length. Expected: 6/18, Actual: {}", b.len());
        }
    } else {
        warn!("Unexpected Peer format: {:?}", d);
    }

    None
}
