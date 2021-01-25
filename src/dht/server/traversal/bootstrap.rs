use crate::dht::bucket::Bucket;
use crate::dht::id::NodeId;
use crate::dht::msg::recv::Response;
use crate::dht::msg::send::FindNode;
use crate::dht::server::traversal::{Status, TraversalNode};
use crate::dht::server::{RpcMgr, Transactions};
use crate::dht::table::RoutingTable;
use std::net::SocketAddr;

pub struct BootstrapTraversal {
    target: NodeId,
    own_id: NodeId,
    nodes: Vec<TraversalNode>,
    txns: Transactions,
    branch_factor: u8,
}

impl BootstrapTraversal {
    pub fn new(target: &NodeId, own_id: &NodeId) -> Self {
        Self {
            target: *target,
            own_id: *own_id,
            nodes: vec![],
            txns: Transactions::new(),
            branch_factor: 3,
        }
    }

    pub async fn start(&mut self, table: &mut RoutingTable, rpc: &mut RpcMgr) {
        trace!("Start BOOTSTRAP traversal");
        let mut closest = Vec::with_capacity(Bucket::MAX_LEN);
        table.find_closest(&self.target, &mut closest, Bucket::MAX_LEN);
        for c in closest {
            self.nodes.push(TraversalNode::new(&c));
        }

        if self.nodes.len() < 3 {
            for node in &table.router_nodes {
                self.nodes.push(TraversalNode {
                    id: NodeId::new(),
                    addr: *node,
                    status: Status::INITIAL | Status::NO_ID,
                });
            }
        }

        self.invoke(rpc).await;
    }

    pub fn prune(&mut self, table: &mut RoutingTable) {
        trace!("Prune BOOTSTRAP traversal");
        let nodes = &mut self.nodes;
        self.txns.prune_with(table, |id| {
            if let Some(node) = nodes.iter_mut().find(|node| &node.id == id) {
                node.status.insert(Status::FAILED);
            }
        })
    }

    pub fn handle_reply(
        &mut self,
        resp: &Response,
        addr: &SocketAddr,
        table: &mut RoutingTable,
    ) -> bool {
        if let Some(req) = self.txns.remove(&resp.txn_id) {
            if req.has_id {
                if &req.id == resp.id {
                    table.heard_from(&req.id);
                } else {
                    warn!("ID mismatch from {}", addr);
                    table.failed(&req.id);
                    return true;
                }
            }
        } else {
            return false;
        }

        if let Some(node) = self.nodes.iter_mut().find(|node| &node.addr == addr) {
            node.status.insert(Status::ALIVE);
        } else {
            return false;
        }

        trace!("Handle BOOTSTRAP traversal response");

        let result = table.read_nodes_with(resp, |c| {
            if !self.nodes.iter().any(|n| &n.id == c.id) {
                self.nodes.push(TraversalNode::new(c));
            }
        });

        if let Err(e) = result {
            warn!("{}", e);
        }

        let target = &self.target;
        self.nodes.sort_by_key(|n| n.id ^ target);
        self.nodes.truncate(100);

        true
    }

    pub async fn invoke(&mut self, rpc: &mut RpcMgr) -> bool {
        trace!("Invoke BOOTSTRAP traversal");
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

            let msg = FindNode {
                target: &self.target,
                id: &self.own_id,
                txn_id: rpc.next_id(),
            };

            match rpc.send(&msg, &n.addr).await {
                Ok(_) => {
                    n.status.insert(Status::QUERIED);
                    self.txns.insert(msg.txn_id, &n.id);
                    outstanding += 1;
                }
                Err(e) => {
                    warn!("{}", e);
                    n.status.insert(Status::FAILED);
                }
            }
        }

        outstanding == 0 && alive == Bucket::MAX_LEN || self.txns.is_empty()
    }

    pub fn done(self) {
        debug!("Done bootstrapping");
    }
}
