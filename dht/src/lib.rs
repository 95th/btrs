#[macro_use]
extern crate log;

#[macro_use]
extern crate anyhow;

#[macro_use]
extern crate bitflags;

mod bucket;
mod contact;
pub mod future;
pub mod id;
pub mod msg;
mod server;
pub mod table;

use id::NodeId;
pub use server::{Client, ClientRequest, Server};
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
