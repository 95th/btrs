use crate::id::NodeId;
use ben::encode::AddBytes;
use ben::{Encode, Encoder};
use std::net::SocketAddr;
use std::time::Instant;

pub struct Peer {
    pub addr: SocketAddr,
}

#[derive(Debug)]
pub struct ContactRef<'a> {
    pub id: &'a NodeId,
    pub addr: SocketAddr,
}

impl ContactRef<'_> {
    pub fn as_owned(&self) -> Contact {
        Contact {
            id: self.id.clone(),
            addr: self.addr,
            last_updated: Instant::now(),
        }
    }
}

#[derive(Debug)]
pub struct Contact {
    pub id: NodeId,
    pub addr: SocketAddr,
    pub last_updated: Instant,
}

impl Contact {
    pub fn new(id: NodeId, addr: SocketAddr) -> Self {
        Self {
            id,
            addr,
            last_updated: Instant::now(),
        }
    }

    pub fn touch(&mut self) {
        self.last_updated = Instant::now();
    }
}

impl Encode for Contact {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        let len = if self.addr.is_ipv4() { 6 } else { 18 };
        let bytes = &mut enc.add_n_bytes(NodeId::LEN + len);
        bytes.add(self.id.as_bytes());
        add_addr(bytes, &self.addr);
    }
}

impl Encode for Peer {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        let len = if self.addr.is_ipv4() { 6 } else { 18 };
        let bytes = &mut enc.add_n_bytes(len);
        add_addr(bytes, &self.addr);
    }
}

fn add_addr<E: Encoder>(bytes: &mut AddBytes<'_, E>, addr: &SocketAddr) {
    match addr {
        SocketAddr::V4(addr) => bytes.add(&addr.ip().octets()),
        SocketAddr::V6(addr) => bytes.add(&addr.ip().octets()),
    }
    bytes.add(&addr.port().to_be_bytes());
}

pub struct CompactNodes<'a> {
    buf: &'a [u8],
}

impl<'a> CompactNodes<'a> {
    pub fn new(buf: &'a [u8]) -> anyhow::Result<Self> {
        ensure!(
            buf.len() % 26 == 0,
            "Compact node list must have length multiple of 26, actual: {}",
            buf.len()
        );

        Ok(Self { buf })
    }
}

impl<'a> Iterator for CompactNodes<'a> {
    type Item = ContactRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.buf.len() < 26 {
            debug_assert!(self.buf.is_empty());
            return None;
        }

        unsafe {
            let p = self.buf.as_ptr();
            let id = &*(p as *const NodeId);
            let addr = &*(p.add(20) as *const [u8; 4]);
            let port = u16::from_be_bytes(*(p.add(24) as *const [u8; 2]));

            self.buf = &self.buf[26..];
            Some(ContactRef {
                id,
                addr: SocketAddr::from((*addr, port)),
            })
        }
    }
}

pub struct CompactNodesV6<'a> {
    buf: &'a [u8],
}

impl<'a> CompactNodesV6<'a> {
    pub fn new(buf: &'a [u8]) -> anyhow::Result<Self> {
        ensure!(
            buf.len() % 38 == 0,
            "Compact node list must have length multiple of 38, actual: {}",
            buf.len()
        );

        Ok(Self { buf })
    }
}

impl<'a> Iterator for CompactNodesV6<'a> {
    type Item = ContactRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.buf.len() < 38 {
            debug_assert!(self.buf.is_empty());
            return None;
        }

        unsafe {
            let p = self.buf.as_ptr();
            let id = &*(p as *const NodeId);
            let addr = &*(p.add(20) as *const [u8; 16]);
            let port = u16::from_be_bytes(*(p.add(36) as *const [u8; 2]));

            self.buf = &self.buf[38..];
            Some(ContactRef {
                id,
                addr: SocketAddr::from((*addr, port)),
            })
        }
    }
}
