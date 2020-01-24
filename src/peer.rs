use rand::distributions::Alphanumeric;
use rand::Rng;
use std::convert::TryInto;
use std::net::{IpAddr, SocketAddr};

#[derive(Debug)]
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

pub fn generate_peer_id() -> String {
    let mut s = String::new();
    s.push('-');
    s.push_str(crate::CLIENT_VERSION);
    s.push('-');
    s.extend(rand::thread_rng().sample_iter(&Alphanumeric).take(12));
    s
}
