use crate::id::NodeId;
use crate::msg::recv::Response;
use crate::msg::send::FindNode;
use crate::server::rpc::Event;
use crate::server::RpcManager;
use crate::table::RoutingTable;
use ben::Encode;
use std::net::SocketAddr;
use std::time::Instant;

use super::base::BaseTask;
use super::{Task, TaskId};

pub struct BootstrapTask {
    base: BaseTask,
}

impl BootstrapTask {
    pub fn new(target: NodeId, table: &mut RoutingTable, task_id: TaskId) -> Self {
        Self {
            base: BaseTask::new(target, table, task_id),
        }
    }
}

impl Task for BootstrapTask {
    fn id(&self) -> TaskId {
        self.base.task_id
    }

    #[instrument(skip_all, fields(task = ?self.id()))]
    fn handle_response(
        &mut self,
        resp: &Response<'_>,
        addr: SocketAddr,
        table: &mut RoutingTable,
        _rpc: &mut RpcManager,
        has_id: bool,
        now: Instant,
    ) {
        trace!("Handle BOOTSTRAP response");
        self.base.handle_response(resp, addr, table, has_id, now);
    }

    fn set_failed(&mut self, id: NodeId, addr: SocketAddr) {
        self.base.set_failed(id, addr);
    }

    #[instrument(skip_all, fields(task = ?self.id()))]
    fn add_requests(&mut self, rpc: &mut RpcManager, now: Instant) -> bool {
        trace!("Add BOOTSTRAP requests");

        let target = self.base.target;
        self.base.add_requests(rpc, now, |buf, rpc| {
            let msg = FindNode {
                txn_id: rpc.new_txn(),
                target,
                id: rpc.own_id,
            };
            trace!("Send {:?}", msg);

            msg.encode(buf);
            msg.txn_id
        })
    }

    fn done(&mut self, rpc: &mut RpcManager) {
        rpc.add_event(Event::Bootstrapped)
    }
}
