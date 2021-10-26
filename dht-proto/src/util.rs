use std::net::{IpAddr, SocketAddr};

pub fn write_addr(buf: &mut Vec<u8>, addr: &SocketAddr) {
    match addr.ip() {
        IpAddr::V4(a) => buf.extend(&a.octets()),
        IpAddr::V6(a) => buf.extend(&a.octets()),
    }
    buf.extend(&addr.port().to_be_bytes());
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
