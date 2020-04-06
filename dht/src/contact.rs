use crate::id::NodeId;
use std::net::Ipv4Addr;
use std::time::Instant;

pub struct Contact {
    pub id: NodeId,
    pub addr: Ipv4Addr,
    pub port: u16,
    pub last_updated: Instant,
}

impl Contact {
    const LEN: usize = NodeId::LEN + 6;

    pub fn new(id: NodeId, addr: Ipv4Addr, port: u16) -> Self {
        Self {
            id,
            addr,
            port,
            last_updated: Instant::now(),
        }
    }

    pub fn as_bytes(&self) -> [u8; Self::LEN] {
        let mut buf = [0; Self::LEN];

        buf[..NodeId::LEN].copy_from_slice(self.id.as_bytes());
        buf[NodeId::LEN..NodeId::LEN + 4].copy_from_slice(&self.addr.octets());
        buf[NodeId::LEN + 4..].copy_from_slice(&self.port.to_be_bytes());

        buf
    }

    pub fn touch(&mut self) {
        self.last_updated = Instant::now();
    }
}
