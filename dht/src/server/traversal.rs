use crate::contact::ContactRef;
use crate::id::NodeId;
use crate::msg::recv::Response;
use crate::server::RpcMgr;
use crate::table::RoutingTable;
use std::net::SocketAddr;

mod bootstrap;
mod get_peers;

pub use bootstrap::BootstrapTraversal;
pub use get_peers::GetPeersTraversal;

pub struct TraversalNode {
    id: NodeId,
    addr: SocketAddr,
    status: Status,
}

impl TraversalNode {
    fn new(c: &ContactRef) -> Self {
        Self {
            id: *c.id,
            addr: c.addr,
            status: Status::INITIAL,
        }
    }
}

bitflags! {
    pub struct Status: u8 {
        const INITIAL   = 0x01;
        const ALIVE     = 0x02;
        const FAILED    = 0x04;
        const NO_ID     = 0x08;
        const QUERIED   = 0x10;
    }
}

pub enum Traversal {
    GetPeers(Box<GetPeersTraversal>),
    Bootstrap(Box<BootstrapTraversal>),
}

impl Traversal {
    pub fn prune(&mut self, table: &mut RoutingTable) {
        match self {
            Self::GetPeers(t) => t.prune(table),
            Self::Bootstrap(t) => t.prune(table),
        }
    }

    pub fn handle_reply(
        &mut self,
        resp: &Response,
        addr: &SocketAddr,
        table: &mut RoutingTable,
    ) -> bool {
        match self {
            Self::GetPeers(t) => t.handle_reply(resp, addr, table),
            Self::Bootstrap(t) => t.handle_reply(resp, addr, table),
        }
    }

    pub async fn invoke(&mut self, rpc: &mut RpcMgr) -> bool {
        match self {
            Self::GetPeers(t) => t.invoke(rpc).await,
            Self::Bootstrap(t) => t.invoke(rpc).await,
        }
    }

    pub fn done(self) {
        match self {
            Self::GetPeers(t) => t.done(),
            Self::Bootstrap(t) => t.done(),
        }
    }
}
