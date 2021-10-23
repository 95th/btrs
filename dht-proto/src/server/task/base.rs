use std::net::SocketAddr;

use crate::{
    bucket::Bucket,
    id::NodeId,
    msg::{recv::Response, TxnId},
    server::rpc::RpcManager,
    table::RoutingTable,
};

use super::{DhtNode, Status, TaskId};

pub struct BaseTask {
    pub target: NodeId,
    pub nodes: Vec<DhtNode>,
    pub branch_factor: u8,
    pub invoke_count: u8,
    pub task_id: TaskId,
}

impl BaseTask {
    pub fn new(target: &NodeId, table: &RoutingTable, task_id: TaskId) -> Self {
        let mut closest = Vec::with_capacity(Bucket::MAX_LEN);
        table.find_closest(target, &mut closest, Bucket::MAX_LEN);

        let mut nodes = vec![];
        for c in closest {
            nodes.push(DhtNode::new(&c));
        }

        if nodes.len() < 3 {
            for node in &table.router_nodes {
                nodes.push(DhtNode {
                    id: NodeId::new(),
                    addr: *node,
                    status: Status::INITIAL | Status::NO_ID,
                });
            }
        }

        Self {
            target: *target,
            nodes,
            branch_factor: 3,
            invoke_count: 0,
            task_id,
        }
    }

    pub fn handle_response(
        &mut self,
        resp: &Response<'_>,
        addr: &SocketAddr,
        table: &mut RoutingTable,
        has_id: bool,
    ) {
        log::trace!("Invoke count: {}", self.invoke_count);
        if has_id {
            if let Some(node) = self.nodes.iter_mut().find(|node| &node.id == resp.id) {
                node.status.insert(Status::ALIVE);
                self.invoke_count -= 1;
            } else {
                log::warn!(
                    "Received a response, but no corresponding DHT node found. {:?}",
                    resp
                );
                return;
            }
        } else if let Some(node) = self.nodes.iter_mut().find(|node| &node.addr == addr) {
            node.id = *resp.id;
            node.status.insert(Status::ALIVE);
            self.invoke_count -= 1;
        }

        let result = table.read_nodes_with(resp, |c| {
            if !self.nodes.iter().any(|n| &n.id == c.id) {
                self.nodes.push(DhtNode::new(c));
            }
        });

        if let Err(e) = result {
            log::warn!("{}", e);
        }

        let target = &self.target;
        self.nodes.sort_by_key(|n| n.id ^ target);

        if self.nodes.len() > 100 {
            for n in &self.nodes[100..] {
                if n.status & (Status::QUERIED | Status::ALIVE | Status::FAILED) == Status::QUERIED
                {
                    self.invoke_count -= 1;
                }
            }
        }

        self.nodes.truncate(100);
        log::trace!("Invoke count after: {}", self.invoke_count);
    }

    pub fn set_failed(&mut self, id: &NodeId, addr: &SocketAddr) {
        if let Some(node) = self
            .nodes
            .iter_mut()
            .find(|node| &node.id == id || &node.addr == addr)
        {
            node.status.insert(Status::FAILED);
            self.invoke_count -= 1;
        }
    }

    pub fn add_requests<F>(&mut self, rpc: &mut RpcManager, mut write_msg: F) -> bool
    where
        F: FnMut(&mut Vec<u8>, &mut RpcManager) -> TxnId,
    {
        let mut outstanding = 0;
        let mut alive = 0;

        for n in &mut self.nodes {
            if alive == Bucket::MAX_LEN {
                break;
            }

            if outstanding == self.branch_factor {
                break;
            }

            if n.status.contains(Status::ALIVE) {
                alive += 1;
                continue;
            }

            if n.status.contains(Status::QUERIED) {
                if !n.status.contains(Status::FAILED) {
                    outstanding += 1;
                }
                continue;
            };

            let mut buf = Vec::new();
            let txn_id = write_msg(&mut buf, rpc);
            log::trace!("Send to {}", n.addr);

            rpc.transmit(self.task_id, n.id, buf, n.addr);
            n.status.insert(Status::QUERIED);
            rpc.txns.insert(txn_id, &n.id, &n.addr, self.task_id);

            outstanding += 1;
            self.invoke_count += 1;
        }

        log::trace!(
            "Outstanding: {}, alive; {}, invoke_count: {}",
            outstanding,
            alive,
            self.invoke_count
        );
        (outstanding == 0 && alive == Bucket::MAX_LEN) || self.invoke_count == 0
    }
}
