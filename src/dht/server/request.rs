use crate::dht::contact::ContactRef;
use crate::dht::id::NodeId;
use crate::dht::msg::recv::Response;
use crate::dht::server::RpcMgr;
use crate::dht::table::RoutingTable;
use std::net::SocketAddr;

mod announce;
mod bootstrap;
mod get_peers;
mod ping;

pub use announce::AnnounceRequest;
pub use bootstrap::BootstrapRequest;
pub use get_peers::GetPeersRequest;
pub use ping::PingRequest;

use super::PeerSender;

pub struct DhtNode {
    id: NodeId,
    addr: SocketAddr,
    status: Status,
}

impl DhtNode {
    fn new(c: &ContactRef) -> Self {
        Self {
            id: *c.id,
            addr: c.addr,
            status: Status::INITIAL,
        }
    }
}

bitflags::bitflags! {
    pub struct Status: u8 {
        const INITIAL   = 0x01;
        const ALIVE     = 0x02;
        const FAILED    = 0x04;
        const NO_ID     = 0x08;
        const QUERIED   = 0x10;
    }
}

pub enum DhtRequest {
    Bootstrap(Box<BootstrapRequest>),
    GetPeers(Box<GetPeersRequest>),
    Announce(Box<AnnounceRequest>),
    Ping(Box<PingRequest>),
}

impl DhtRequest {
    pub fn new_bootstrap(target: &NodeId, own_id: &NodeId, table: &mut RoutingTable) -> Self {
        Self::Bootstrap(Box::new(BootstrapRequest::new(target, own_id, table)))
    }

    pub fn new_get_peers(
        info_hash: &NodeId,
        own_id: &NodeId,
        tx: PeerSender,
        table: &mut RoutingTable,
    ) -> Self {
        Self::GetPeers(Box::new(GetPeersRequest::new(info_hash, own_id, tx, table)))
    }

    pub fn new_announce(
        info_hash: &NodeId,
        own_id: &NodeId,
        tx: PeerSender,
        table: &mut RoutingTable,
    ) -> Self {
        Self::Announce(Box::new(AnnounceRequest::new(info_hash, own_id, tx, table)))
    }

    pub fn new_ping(own_id: &NodeId, id: &NodeId, addr: &SocketAddr) -> Self {
        Self::Ping(Box::new(PingRequest::new(own_id, id, addr)))
    }

    pub fn prune(&mut self, table: &mut RoutingTable) {
        match self {
            Self::Bootstrap(t) => t.prune(table),
            Self::GetPeers(t) => t.prune(table),
            Self::Announce(t) => t.prune(table),
            Self::Ping(t) => t.prune(table),
        }
    }

    /// Handle an incoming response and return `true` if it
    /// was handled in this request.
    /// Returning `false` means that the response didn't belong
    /// to this request.
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
