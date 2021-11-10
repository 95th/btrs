use crate::{
    id::NodeId,
    util::{self, WithBytes},
};
use ben::{Encode, LazyBytesEncoder};
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

#[derive(Debug, Clone, PartialEq)]
pub struct Contact {
    pub id: NodeId,
    pub addr: SocketAddr,
    pub status: ContactStatus,
    timeout_count: Option<u8>,
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

    pub fn write_compact(&self, buf: &mut Vec<u8>) {
        buf.extend(&self.id[..]);
        util::write_addr(buf, self.addr);
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

    pub fn set_confirmed(&mut self) {
        self.timeout_count = Some(0);
    }

    pub fn is_confirmed(&self) -> bool {
        matches!(self.timeout_count, Some(0))
    }
}

impl Encode for Contact {
    fn encode(&self, buf: &mut Vec<u8>) {
        let mut bytes = LazyBytesEncoder::<38>::new(buf);
        bytes.extend(self.id);
        self.addr.ip().with_bytes(|b| bytes.extend(b));
        bytes.extend(self.addr.port().to_be_bytes());
    }
}

#[repr(C)]
struct CompactNode<const N: usize> {
    id: NodeId,
    ip: [u8; N],
    port: [u8; 2],
}

pub struct CompactNodeIter<'a, const N: usize> {
    iter: std::slice::Iter<'a, CompactNode<N>>,
}

impl<'a, const N: usize> CompactNodeIter<'a, N> {
    pub fn new(buf: &'a [u8]) -> anyhow::Result<Self> {
        let size = std::mem::size_of::<CompactNode<N>>();

        anyhow::ensure!(
            buf.len() % size == 0,
            "Compact node list must have length multiple of {}, actual: {}",
            size,
            buf.len()
        );

        let iter = unsafe {
            let ptr = buf.as_ptr().cast::<CompactNode<N>>();
            let slice = std::slice::from_raw_parts(ptr, buf.len() / size);
            slice.iter()
        };

        Ok(Self { iter })
    }
}

impl<'a> Iterator for CompactNodeIter<'a, 4> {
    type Item = Contact;

    fn next(&mut self) -> Option<Self::Item> {
        let node = self.iter.next()?;
        let port = u16::from_be_bytes(node.port);
        let addr = SocketAddr::from((node.ip, port));

        Some(Contact::new(node.id, addr))
    }
}

impl<'a> Iterator for CompactNodeIter<'a, 16> {
    type Item = Contact;

    fn next(&mut self) -> Option<Self::Item> {
        let node = self.iter.next()?;
        let port = u16::from_be_bytes(node.port);
        let addr = SocketAddr::from((node.ip, port));

        Some(Contact::new(node.id, addr))
    }
}
