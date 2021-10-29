use crate::contact::Contact;
use crate::id::NodeId;
use crate::{msg::recv::Response, table::RoutingTable};
use std::net::SocketAddr;
use std::time::Instant;

mod announce;
mod base;
mod bootstrap;
mod get_peers;
mod ping;

pub use announce::AnnounceTask;
pub use bootstrap::BootstrapTask;
pub use get_peers::GetPeersTask;
pub use ping::PingTask;

use super::rpc::RpcManager;

pub trait Task {
    fn id(&self) -> TaskId;

    fn add_requests(&mut self, rpc: &mut RpcManager, now: Instant) -> bool;

    fn set_failed(&mut self, id: NodeId, addr: SocketAddr);

    fn handle_response(
        &mut self,
        resp: &Response<'_>,
        addr: SocketAddr,
        table: &mut RoutingTable,
        rpc: &mut RpcManager,
        has_id: bool,
        now: Instant,
    );

    fn done(&mut self, _rpc: &mut RpcManager) {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskId(pub(crate) usize);

pub struct DhtNode {
    pub id: NodeId,
    pub key: NodeId,
    pub addr: SocketAddr,
    pub status: Status,
}

impl DhtNode {
    pub fn new(c: &Contact, target: NodeId) -> Self {
        Self {
            id: c.id,
            key: c.id ^ target,
            addr: c.addr,
            status: Status::INITIAL,
        }
    }

    pub fn set_id(&mut self, id: NodeId, target: NodeId) {
        self.id = id;
        self.key = id ^ target;
    }
}

bitflags::bitflags! {
    pub struct Status: u8 {
        const INITIAL   = 1 << 0;
        const ALIVE     = 1 << 1;
        const FAILED    = 1 << 2;
        const NO_ID     = 1 << 3;
        const QUERIED   = 1 << 4;
    }
}
