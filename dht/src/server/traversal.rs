use crate::contact::ContactRef;
use crate::id::NodeId;
use crate::msg::recv::Response;
use crate::server::RpcMgr;
use crate::table::RoutingTable;
use std::net::SocketAddr;

mod announce;
mod bootstrap;
mod get_peers;
mod ping;

pub use announce::AnnounceTraversal;
pub use bootstrap::BootstrapTraversal;
pub use get_peers::GetPeersTraversal;
pub use ping::PingTraversal;

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
    Bootstrap(Box<BootstrapTraversal>),
    GetPeers(Box<GetPeersTraversal>),
    Announce(Box<AnnounceTraversal>),
    Ping(Box<PingTraversal>),
}

impl Traversal {
    pub fn prune(&mut self, table: &mut RoutingTable) {
        match self {
            Self::Bootstrap(t) => t.prune(table),
            Self::GetPeers(t) => t.prune(table),
            Self::Announce(t) => t.prune(table),
            Self::Ping(t) => t.prune(table),
        }
    }

    /// Handle an incoming response and return `true` if it
    /// was handled in this traversal.
    /// Returning `false` means that the response didn't belong
    /// to this traversal.
    pub fn handle_reply(
        &mut self,
        resp: &Response,
        addr: &SocketAddr,
        table: &mut RoutingTable,
    ) -> bool {
        match self {
            Self::Bootstrap(t) => t.handle_reply(resp, addr, table),
            Self::GetPeers(t) => t.handle_reply(resp, addr, table),
            Self::Announce(t) => t.handle_reply(resp, addr, table),
            Self::Ping(t) => t.handle_reply(resp, addr, table),
        }
    }

    pub async fn invoke(&mut self, rpc: &mut RpcMgr) -> bool {
        match self {
            Self::Bootstrap(t) => t.invoke(rpc).await,
            Self::GetPeers(t) => t.invoke(rpc).await,
            Self::Announce(t) => t.invoke(rpc).await,
            Self::Ping(t) => t.invoke(rpc).await,
        }
    }

    pub fn done(self) {
        match self {
            Self::Bootstrap(t) => t.done(),
            Self::GetPeers(t) => t.done(),
            Self::Announce(t) => t.done(),
            Self::Ping(t) => t.done(),
        }
    }
}
