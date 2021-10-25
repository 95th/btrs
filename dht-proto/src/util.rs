use std::net::{IpAddr, SocketAddr};

pub fn write_addr(buf: &mut Vec<u8>, addr: &SocketAddr) {
    match addr.ip() {
        IpAddr::V4(a) => buf.extend(&a.octets()),
        IpAddr::V6(a) => buf.extend(&a.octets()),
    }
    buf.extend(&addr.port().to_be_bytes());
}
