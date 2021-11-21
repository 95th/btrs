use client::PeerId;
use rand::{distributions::Alphanumeric, Rng};
use std::net::SocketAddr;

pub fn v4(bytes: &[u8]) -> SocketAddr {
    let ip: [u8; 4] = bytes[..4].try_into().unwrap();
    let port_bytes: [u8; 2] = bytes[4..].try_into().unwrap();
    SocketAddr::from((ip, u16::from_be_bytes(port_bytes)))
}

pub fn v6(bytes: &[u8]) -> SocketAddr {
    let ip: [u8; 16] = bytes[..16].try_into().unwrap();
    let port_bytes: [u8; 2] = bytes[16..].try_into().unwrap();
    SocketAddr::from((ip, u16::from_be_bytes(port_bytes)))
}

pub fn generate_peer_id() -> PeerId {
    let mut buf = *b"-UT3100-000000000000";
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .zip(&mut buf[8..])
        .for_each(|(c, b)| *b = c as u8);
    buf
}
