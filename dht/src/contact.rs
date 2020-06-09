use crate::id::NodeId;
use ben::{Encode, Encoder};
use std::net::Ipv4Addr;
use std::time::Instant;

pub struct Peer {
    pub addr: Ipv4Addr,
    pub port: u16,
}

#[derive(Debug)]
pub struct ContactRef<'a> {
    pub id: &'a NodeId,
    pub addr: Ipv4Addr,
    pub port: u16,
}

impl ContactRef<'_> {
    pub fn to_owned(self) -> Contact {
        Contact {
            id: self.id.clone(),
            addr: self.addr,
            port: self.port,
            last_updated: Instant::now(),
        }
    }
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
        if self.buf.len() == 0 {
            return None;
        }

        let id = unsafe { buf_as::<NodeId>(&self.buf[..20]) };

        let addr = unsafe { buf_as::<[u8; 4]>(&self.buf[20..24]) };
        let addr = Ipv4Addr::from(*addr);

        let port = unsafe { buf_as::<[u8; 2]>(&self.buf[24..26]) };
        let port = u16::from_be_bytes(*port);

        self.buf = &self.buf[26..];
        Some(ContactRef { id, addr, port })
    }
}

unsafe fn buf_as<T>(buf: &[u8]) -> &T {
    debug_assert_eq!(std::mem::size_of::<T>(), buf.len());
    debug_assert_eq!(std::mem::align_of::<T>(), 1);
    let p = buf.as_ptr() as *const T;
    &*p
}
