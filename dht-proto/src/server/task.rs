use crate::id::NodeId;
use crate::{contact::ContactRef, msg::recv::Response, table::RoutingTable};
use std::net::SocketAddr;

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

pub trait Task: Send {
    fn id(&self) -> TaskId;

    fn add_requests(&mut self, rpc: &mut RpcManager) -> bool;

    fn set_failed(&mut self, id: &NodeId, addr: &SocketAddr);

    fn handle_response(
        &mut self,
        resp: &Response<'_>,
        addr: &SocketAddr,
        table: &mut RoutingTable,
        rpc: &mut RpcManager,
        has_id: bool,
    );

    fn done(&mut self, _rpc: &mut RpcManager) {}
}

pub type TaskId = usize;

pub struct DhtNode {
    pub id: NodeId,
    pub addr: SocketAddr,
    pub status: Status,
}

impl DhtNode {
    pub fn new(c: &ContactRef) -> Self {
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
