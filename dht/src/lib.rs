#[macro_use]
extern crate anyhow;

mod bucket;
mod contact;
pub mod id;
pub mod msg;
pub mod table;

use id::NodeId;
use table::RoutingTable;

pub struct Kademlia {
    routes: RoutingTable,
}

impl Kademlia {
    pub fn new(id: NodeId) -> Self {
        Self {
            routes: RoutingTable::new(id),
        }
    }
}
