use crate::bitfield::BitField;
use rand::distributions::Alphanumeric;
use rand::Rng;
use std::convert::TryInto;
use std::net::{IpAddr, SocketAddr};

pub type PeerId = [u8; 20];

#[derive(Debug, Clone, Copy)]
pub struct PeerStatus {
    pub choked: bool,
    pub interested: bool,
}

impl Default for PeerStatus {
    fn default() -> Self {
        Self {
            choked: true,
            interested: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Peer {
    addr: SocketAddr,
    pieces: BitField,
    local_status: PeerStatus,
    remote_status: PeerStatus,
}

impl Peer {
    pub fn new(ip: IpAddr, port: u16) -> Self {
        SocketAddr::new(ip, port).into()
    }

    pub fn v4(bytes: &[u8]) -> Self {
        let ip: [u8; 4] = bytes[..4].try_into().unwrap();
        let port_bytes: [u8; 2] = bytes[4..].try_into().unwrap();
        Self::new(ip.into(), u16::from_be_bytes(port_bytes))
    }

    pub fn v6(bytes: &[u8]) -> Self {
        let ip: [u8; 16] = bytes[..16].try_into().unwrap();
        let port_bytes: [u8; 2] = bytes[16..].try_into().unwrap();
        Self::new(ip.into(), u16::from_be_bytes(port_bytes))
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
}

impl From<SocketAddr> for Peer {
    fn from(addr: SocketAddr) -> Self {
        Self {
            addr,
            pieces: Default::default(),
            local_status: Default::default(),
            remote_status: Default::default(),
        }
    }
}

pub fn generate_peer_id() -> PeerId {
    let mut buf = [0; 20];
    buf[0] = b'-';
    buf[1..7].copy_from_slice(crate::CLIENT_VERSION);
    buf[7] = b'-';
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .zip(&mut buf[7..])
        .for_each(|(c, b)| *b = c as u8);
    buf
}
