use std::net::{IpAddr, SocketAddr};

pub fn write_addr(buf: &mut Vec<u8>, addr: &SocketAddr) {
    addr.ip().with_bytes(|b| buf.extend(b));
    buf.extend(&addr.port().to_be_bytes());
}

pub trait WithBytes {
    fn with_bytes<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R;
}

impl WithBytes for IpAddr {
    fn with_bytes<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R {
        match self {
            IpAddr::V4(ip) => f(&ip.octets()),
            IpAddr::V6(ip) => f(&ip.octets()),
        }
    }
}

pub struct ArrayReader<'a> {
    buf: &'a [u8],
}

impl<'a> ArrayReader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    pub fn read<const N: usize>(&mut self) -> Option<&'a [u8; N]> {
        if self.buf.len() >= N {
            let p = self.buf.as_ptr().cast();
            self.buf = &self.buf[N..];
            unsafe { Some(&*p) }
        } else {
            None
        }
    }
}
