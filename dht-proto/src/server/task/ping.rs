use crate::contact::Contact;
use crate::id::NodeId;
use crate::msg::recv::Response;
use crate::msg::send::Ping;
use crate::server::task::{DhtNode, Status};
use crate::server::RpcManager;
use crate::table::RoutingTable;
use ben::Encode;
use std::net::SocketAddr;
use std::time::Instant;

use super::{Task, TaskId};

pub struct PingTask {
    node: DhtNode,
    done: bool,
    task_id: TaskId,
}

impl PingTask {
    pub fn new(id: NodeId, addr: SocketAddr, task_id: TaskId) -> Self {
        Self {
            node: DhtNode {
                id,
                key: id,
                addr,
                status: Status::INITIAL,
            },
            done: false,
            task_id,
        }
    }
}

impl Task for PingTask {
    fn id(&self) -> TaskId {
        self.task_id
    }

    fn handle_response(
        &mut self,
        resp: &Response<'_>,
        addr: SocketAddr,
        table: &mut RoutingTable,
        _rpc: &mut RpcManager,
        _has_id: bool,
        now: Instant,
    ) {
        log::trace!("Handle PING response");

        if self.node.id == resp.id && self.node.addr == addr {
            table.add_contact(Contact::new(resp.id, addr), now);
        } else {
            table.failed(resp.id);
        }

        self.done = true;
    }

    fn set_failed(&mut self, id: NodeId, _addr: SocketAddr) {
        if self.node.id == id {
            self.node.status.insert(Status::FAILED);
        }
        self.done = true;
    }

    fn add_requests(&mut self, rpc: &mut RpcManager, now: Instant) -> bool {
        log::trace!("Invoke PING request");
        if self.done {
            return true;
        }

        let txn_id = rpc.new_txn();

        let mut buf = Vec::new();
        let msg = Ping {
            txn_id,
            id: rpc.own_id,
        };

        msg.encode(&mut buf);

        rpc.transmit(self.id(), self.node.id, buf, self.node.addr);
        self.node.status.insert(Status::QUERIED);
        rpc.txns
            .insert(txn_id, self.node.id, self.node.addr, self.task_id, now);
        false
    }
}
