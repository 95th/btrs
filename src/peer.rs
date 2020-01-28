use rand::distributions::Alphanumeric;
use rand::Rng;
use std::convert::TryInto;
use std::net::{IpAddr, SocketAddr};

pub type PeerId = [u8; 20];

#[derive(Debug, Clone)]
pub struct Peer {
    ip: IpAddr,
    port: u16,
}

impl Peer {
    pub fn new(ip: IpAddr, port: u16) -> Self {
        Self { ip, port }
    }

    pub fn v4(bytes: &[u8]) -> Self {
        let ip: [u8; 4] = bytes[..4].try_into().unwrap();
        let port = u16::from_be_bytes(bytes[4..].try_into().unwrap());
        Self {
            ip: ip.into(),
            port,
        }
    }

    pub fn v6(bytes: &[u8]) -> Self {
        let ip: [u8; 16] = bytes[..16].try_into().unwrap();
        let port = u16::from_be_bytes(bytes[16..].try_into().unwrap());
        Self {
            ip: ip.into(),
            port,
        }
    }

    pub fn addr(&self) -> SocketAddr {
        SocketAddr::new(self.ip, self.port)
    }
}

impl From<SocketAddr> for Peer {
    fn from(addr: SocketAddr) -> Self {
        Self {
            ip: addr.ip(),
            port: addr.port(),
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
