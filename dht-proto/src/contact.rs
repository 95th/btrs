use crate::{
    id::NodeId,
    util::{self, to_ipv6},
};
use ben::Encode;
use std::net::SocketAddr;

bitflags::bitflags! {
    pub struct ContactStatus: u8 {
        const QUERIED       = 1 << 0;
        const INITIAL       = 1 << 1;
        const NO_ID         = 1 << 2;
        const SHORT_TIMEOUT = 1 << 3;
        const FAILED        = 1 << 4;
        const IPV6_ADDRESS  = 1 << 5;
        const ALIVE         = 1 << 6;
        const DONE          = 1 << 7;
    }
}

#[derive(Debug)]
pub struct ContactRef<'a> {
    pub id: &'a NodeId,
    pub addr: SocketAddr,
}

impl ContactRef<'_> {
    pub fn as_owned(&self) -> Contact {
        Contact::new(*self.id, self.addr)
    }

    pub fn write_compact(&self, buf: &mut Vec<u8>) {
        buf.extend(&self.id[..]);
        util::write_addr(buf, &self.addr);
    }
}

#[derive(Debug, Clone)]
pub struct Contact {
    pub id: NodeId,
    pub addr: SocketAddr,
    pub status: ContactStatus,
    pub timeout_count: Option<u8>,
}

impl Contact {
    pub fn new(id: NodeId, addr: SocketAddr) -> Self {
        Self {
            id,
            addr,
            timeout_count: None,
            status: ContactStatus::INITIAL,
        }
    }

    pub fn as_ref(&self) -> ContactRef<'_> {
        ContactRef {
            id: &self.id,
            addr: self.addr,
        }
    }

    pub fn is_pinged(&self) -> bool {
        self.timeout_count.is_some()
    }

    pub fn set_pinged(&mut self) {
        if self.timeout_count.is_none() {
            self.timeout_count = Some(0);
        }
    }

    pub fn timed_out(&mut self) {
        if let Some(c) = &mut self.timeout_count {
            *c = c.saturating_add(1);
        }
    }

    pub fn fail_count(&self) -> u8 {
        self.timeout_count.unwrap_or(0)
    }

    pub fn failed(&self) -> bool {
        self.fail_count() > 0
    }

    pub fn clear_timeout(&mut self) {
        if let Some(c) = &mut self.timeout_count {
            *c = 0;
        }
    }

    pub fn is_confirmed(&self) -> bool {
        matches!(self.timeout_count, Some(0))
    }
}

impl Encode for Contact {
    fn encode(&self, buf: &mut Vec<u8>) {
        let mut bytes = [0; 38];
        bytes[..20].copy_from_slice(&self.id[..]);

        match &self.addr {
            SocketAddr::V4(addr) => {
                bytes[20..24].copy_from_slice(&addr.ip().octets());
                bytes[24..26].copy_from_slice(&addr.port().to_be_bytes());
                bytes[..26].encode(buf);
            }
            SocketAddr::V6(addr) => {
                bytes[20..36].copy_from_slice(&addr.ip().octets());
                bytes[36..38].copy_from_slice(&addr.port().to_be_bytes());
                bytes.encode(buf);
            }
        }
    }
}

pub struct CompactNodes<'a> {
    buf: &'a [u8],
}

impl<'a> CompactNodes<'a> {
    pub fn new(buf: &'a [u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(
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
            let id = &*p.cast::<NodeId>();
            let addr = &*p.add(20).cast::<[u8; 4]>();
            let port = u16::from_be_bytes(*p.add(36).cast());

            self.buf = &self.buf[26..];
            Some(ContactRef {
                id,
                addr: to_ipv6(SocketAddr::from((*addr, port))),
            })
        }
    }
}

pub struct CompactNodesV6<'a> {
    buf: &'a [u8],
}

impl<'a> CompactNodesV6<'a> {
    pub fn new(buf: &'a [u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(
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
            let id = &*p.cast::<NodeId>();
            let addr = &*p.add(20).cast::<[u8; 16]>();
            let port = u16::from_be_bytes(*p.add(36).cast());

            self.buf = &self.buf[38..];
            Some(ContactRef {
                id,
                addr: SocketAddr::from((*addr, port)),
            })
        }
    }
}
