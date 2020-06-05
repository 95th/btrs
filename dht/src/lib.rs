mod bucket;
mod contact;
pub mod id;
pub mod msg;
pub mod table;

use id::NodeId;
use table::RoutingTable;

pub struct Dht {
    pub routes: RoutingTable,
}

impl Dht {
    pub fn new(id: NodeId) -> Self {
        Self {
            routes: RoutingTable::new(id),
        }
    }
}
