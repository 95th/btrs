use std::{net::SocketAddr, time::Instant};

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
    pub invoked: u8,
    pub task_id: TaskId,
}

impl BaseTask {
    pub fn new(target: &NodeId, table: &RoutingTable, task_id: TaskId) -> Self {
        let closest = table.find_closest(target, Bucket::MAX_LEN);

        let mut nodes = vec![];
        for c in closest {
            nodes.push(DhtNode::new(c, target));
        }

        log::info!("Closest nodes in the routing table: {}", nodes.len());

        if nodes.len() < 3 {
            for node in &table.router_nodes {
                nodes.push(DhtNode {
                    id: NodeId::new(),
                    key: *target,
                    addr: *node,
                    status: Status::INITIAL | Status::NO_ID,
                });
            }
        }

        nodes.sort_unstable_by_key(|n| n.key);

        Self {
            target: *target,
            nodes,
            branch_factor: 3,
            invoked: 0,
            task_id,
        }
    }

    pub fn handle_response(
        &mut self,
        resp: &Response<'_>,
        addr: &SocketAddr,
        table: &mut RoutingTable,
        has_id: bool,
        now: Instant,
    ) {
        log::trace!("Invoked before: {}", self.invoked);
        if has_id {
            let key = resp.id ^ self.target;
            let result = self.nodes.binary_search_by_key(&key, |n| n.key);

            if let Ok(i) = result {
                self.nodes[i].status.insert(Status::ALIVE);
                self.invoked -= 1;
            } else {
                log::warn!(
                    "Received a response, but no corresponding DHT node found. {:?}",
                    resp
                );
                return;
            }
        } else if let Some(node) = self.nodes.iter_mut().find(|node| &node.addr == addr) {
            node.set_id(resp.id, &self.target);
            node.status.insert(Status::ALIVE);
            self.nodes.sort_unstable_by_key(|n| n.key);
            self.invoked -= 1;
        }

        let result = table.read_nodes_with(resp, now, |c| {
            let key = c.id ^ self.target;
            let search_result = self.nodes.binary_search_by_key(&key, |n| n.key);

            // Insert if not present
            if let Err(i) = search_result {
                self.nodes.insert(i, DhtNode::with_ref(c, &self.target));
            }
        });

        if let Err(e) = result {
            log::warn!("{}", e);
        }

        if self.nodes.len() > 100 {
            let mask = Status::QUERIED | Status::ALIVE | Status::FAILED;

            for n in &self.nodes[100..] {
                if n.status & mask == Status::QUERIED {
                    self.invoked -= 1;
                }
            }
        }

        self.nodes.truncate(100);

        log::trace!("Invoked after: {}", self.invoked);
    }

    pub fn set_failed(&mut self, id: &NodeId, addr: &SocketAddr) {
        let key = id ^ self.target;
        if let Ok(i) = self.nodes.binary_search_by_key(&key, |n| n.key) {
            let node = &mut self.nodes[i];
            node.status.insert(Status::FAILED);
            self.invoked -= 1;
        } else if let Some(node) = self.nodes.iter_mut().find(|n| n.addr == *addr) {
            node.status.insert(Status::FAILED);
            self.invoked -= 1;
        }
    }

    pub fn add_requests<F>(&mut self, rpc: &mut RpcManager, now: Instant, mut write_msg: F) -> bool
    where
        F: FnMut(&mut Vec<u8>, &mut RpcManager) -> TxnId,
    {
        let mut pending = 0;
        let mut alive = 0;

        // If newer nodes are found and a pending node falls out of the `branch_factor` window,
        // it is not considered pending anymore and a new request can be made.

        for n in &mut self.nodes {
            if alive == Bucket::MAX_LEN {
                break;
            }

            if pending == self.branch_factor {
                break;
            }

            if n.status.contains(Status::ALIVE) {
                alive += 1;
                continue;
            }

            if n.status.contains(Status::QUERIED) {
                if !n.status.contains(Status::FAILED) {
                    pending += 1;
                }
                continue;
            };

            let mut buf = Vec::new();
            let txn_id = write_msg(&mut buf, rpc);
            log::trace!("Send to {}", n.addr);

            rpc.transmit(self.task_id, n.id, buf, n.addr);
            n.status.insert(Status::QUERIED);
            rpc.txns.insert(txn_id, &n.id, &n.addr, self.task_id, now);

            pending += 1;
            self.invoked += 1;
        }

        log::trace!(
            "Pending: {}, alive; {}, Invoked: {}",
            pending,
            alive,
            self.invoked
        );

        // We are done when there are no pending nodes and we found `k` alive nodes
        // OR there are no queried nodes
        (pending == 0 && alive == Bucket::MAX_LEN) || self.invoked == 0
    }
}
