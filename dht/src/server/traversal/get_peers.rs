use crate::bucket::Bucket;
use crate::id::NodeId;
use crate::msg::recv::Response;
use crate::msg::send::GetPeers;
use crate::server::traversal::{Status, TraversalNode};
use crate::server::{PeerSender, RpcMgr, Transactions};
use crate::table::RoutingTable;
use ben::Decoder;
use std::net::SocketAddr;

pub struct GetPeersTraversal {
    info_hash: NodeId,
    own_id: NodeId,
    nodes: Vec<TraversalNode>,
    peers: Vec<SocketAddr>,
    txns: Transactions,
    tx: PeerSender,
    branch_factor: u8,
}

impl GetPeersTraversal {
    pub fn new(info_hash: &NodeId, own_id: &NodeId, tx: PeerSender) -> Self {
        Self {
            info_hash: *info_hash,
            own_id: *own_id,
            nodes: vec![],
            peers: vec![],
            txns: Transactions::new(),
            tx,
            branch_factor: 3,
        }
    }

    pub async fn start(&mut self, table: &mut RoutingTable, rpc: &mut RpcMgr) {
        trace!("Start GET_PEERS traversal");
        let mut closest = Vec::with_capacity(Bucket::MAX_LEN);
        table.find_closest(&self.info_hash, &mut closest, Bucket::MAX_LEN);
        for c in closest {
            self.nodes.push(TraversalNode::new(&c));
        }

        if self.nodes.len() < 3 {
            for node in &table.router_nodes {
                self.nodes.push(TraversalNode {
                    id: NodeId::gen(),
                    addr: *node,
                    status: Status::INITIAL | Status::NO_ID,
                });
            }
        }

        self.invoke(rpc).await;
    }

    pub fn prune(&mut self, table: &mut RoutingTable) {
        trace!("Prune GET_PEERS traversal");
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
                table.heard_from(&req.id);
            }
        } else {
            return false;
        }

        if let Some(node) = self.nodes.iter_mut().find(|node| &node.addr == addr) {
            node.status.insert(Status::ALIVE);
        } else {
            debug_assert!(false, "Shouldn't be here");
            return false;
        }

        trace!("Handle GET_PEERS traversal response");

        let result = table.read_nodes_with(resp, |c| {
            if !self.nodes.iter().any(|n| &n.id == c.id) {
                self.nodes.push(TraversalNode::new(c));
            }
        });

        fn decode_peer(d: Decoder) -> Option<SocketAddr> {
            if let Some(b) = d.as_bytes() {
                if b.len() == 6 {
                    unsafe {
                        let ip = *(b.as_ptr() as *const [u8; 4]);
                        let port = *(b.as_ptr().add(4) as *const [u8; 2]);
                        let port = u16::from_be_bytes(port);
                        return Some((ip, port).into());
                    }
                } else {
                    warn!("Incorrect Peer length. Expected: 6, Actual: {}", b.len());
                }
            } else {
                warn!("Unexpected Peer format: {:?}", d);
            }

            None
        }

        if let Some(peers) = resp.body.get_list("values") {
            let peers = peers.into_iter().flat_map(decode_peer);
            self.peers.extend(peers);
        }

        if let Err(e) = result {
            warn!("{}", e);
        }

        let target = &self.info_hash;
        self.nodes.sort_by_key(|n| n.id ^ target);
        self.nodes.truncate(100);

        true
    }

    pub async fn invoke(&mut self, rpc: &mut RpcMgr) -> bool {
        trace!("Invoke GET_PEERS traversal");
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

            let msg = GetPeers {
                info_hash: &self.info_hash,
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

        outstanding == 0 && alive == Bucket::MAX_LEN
    }

    pub fn done(self) {
        match self.tx.send(self.peers) {
            Ok(_) => debug!("Replied to GET_PEERS client request"),
            Err(_) => warn!("Reply to GET_PEERS client request failed"),
        }
    }
}
