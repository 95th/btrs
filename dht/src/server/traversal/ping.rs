use crate::contact::ContactRef;
use crate::id::NodeId;
use crate::msg::recv::Response;
use crate::msg::send::Ping;
use crate::msg::TxnId;
use crate::server::traversal::{Status, TraversalNode};
use crate::server::RpcMgr;
use crate::table::RoutingTable;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

pub struct PingTraversal {
    own_id: NodeId,
    node: TraversalNode,
    txn_id: TxnId,
    sent: Instant,
    done: bool,
}

impl PingTraversal {
    pub fn new(own_id: &NodeId, id: &NodeId, addr: &SocketAddr) -> Self {
        Self {
            own_id: *own_id,
            node: TraversalNode {
                id: *id,
                addr: *addr,
                status: Status::INITIAL,
            },
            txn_id: TxnId(0),
            sent: Instant::now(),
            done: false,
        }
    }

    pub fn prune(&mut self, table: &mut RoutingTable) {
        trace!("Prune PING traversal");
        if !self.done && self.sent < Instant::now() - Duration::from_secs(10) {
            table.failed(&self.node.id);
            self.done = true;
        }
    }

    pub fn handle_reply(
        &mut self,
        resp: &Response,
        addr: &SocketAddr,
        table: &mut RoutingTable,
    ) -> bool {
        if self.txn_id != resp.txn_id {
            return false;
        }

        trace!("Handle PING traversal response");

        if self.node.id == *resp.id && self.node.addr == *addr {
            table.add_contact(&ContactRef {
                id: resp.id,
                addr: *addr,
            });
        } else {
            table.failed(resp.id);
        }

        self.done = true;
        true
    }

    pub async fn invoke(&mut self, rpc: &mut RpcMgr) -> bool {
        trace!("Invoke PING traversal");
        if self.done {
            return true;
        }

        let msg = Ping {
            id: &self.own_id,
            txn_id: rpc.next_id(),
        };

        match rpc.send(&msg, &self.node.addr).await {
            Ok(_) => {
                self.node.status.insert(Status::QUERIED);
                self.txn_id = msg.txn_id;
                false
            }
            Err(e) => {
                warn!("{}", e);
                true
            }
        }
    }

    pub fn done(self) {
        debug!("Done Pinging");
    }
}
