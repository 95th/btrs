use crate::id::NodeId;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

pub enum Endpoint {
    Tcp(SocketAddr),
    Udp(SocketAddr),
}

pub struct NodeEntry {
    pub last_queried: Instant,
    pub node_id: NodeId,
    pub endpoint: Endpoint,
    pub rtt: u16,
    pub timeout_count: u8,
    pub verified: bool,
}

impl NodeEntry {
    pub fn new(node_id: NodeId, endpoint: Endpoint) -> Self {
        Self {
            last_queried: Instant::now() - Duration::from_secs(1) * 60 * 60,
            node_id,
            endpoint,
            rtt: 0,
            timeout_count: 0xff,
            verified: false,
        }
    }
}
