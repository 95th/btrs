use crate::id::NodeId;
use ben::{Encode, Encoder};
use std::net::Ipv4Addr;
use std::time::Instant;

pub struct Peer {
    pub addr: Ipv4Addr,
    pub port: u16,
}

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

impl Encode for Contact {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        let mut bytes = enc.add_n_bytes(NodeId::LEN + 6);
        bytes.add(self.id.as_bytes());
        bytes.add(&self.addr.octets());
        bytes.add(&self.port.to_be_bytes());
    }
}

impl Encode for Peer {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        let mut bytes = enc.add_n_bytes(6);
        bytes.add(&self.addr.octets());
        bytes.add(&self.port.to_be_bytes());
    }
}
