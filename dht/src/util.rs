use std::net::{IpAddr, SocketAddr};

pub fn to_ipv6(addr: SocketAddr) -> SocketAddr {
    let ip = match addr.ip() {
        IpAddr::V4(v4) => v4.to_ipv6_mapped(),
        IpAddr::V6(v6) => v6,
    };
    (ip, addr.port()).into()
}

pub fn write_addr(buf: &mut Vec<u8>, addr: &SocketAddr) {
    match addr.ip() {
        IpAddr::V4(a) => buf.extend(&a.octets()),
        IpAddr::V6(a) => buf.extend(&a.octets()),
    }
    buf.extend(&addr.port().to_be_bytes());
}
