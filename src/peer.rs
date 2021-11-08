use rand::{distributions::Alphanumeric, Rng};
use std::{
    convert::TryInto,
    fmt,
    hash::Hash,
    net::{IpAddr, SocketAddr},
};

macro_rules! thin_wrapper {
    ($name:ident, $ty:ty) => {
        #[derive(Default, Debug, Clone)]
        pub struct $name($ty);

        impl $name {
            pub fn new(val: $ty) -> Self {
                Self(val)
            }
        }

        impl std::ops::Deref for $name {
            type Target = $ty;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl std::ops::DerefMut for $name {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }
    };
}

pub use client::PeerId;

thin_wrapper!(Extensions, [u8; 8]);

#[derive(Copy, Clone)]
pub struct Peer {
    pub addr: SocketAddr,
}

impl fmt::Debug for Peer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.addr)
    }
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
}

impl From<SocketAddr> for Peer {
    fn from(addr: SocketAddr) -> Self {
        Self { addr }
    }
}

impl PartialEq for Peer {
    fn eq(&self, other: &Self) -> bool {
        self.addr == other.addr
    }
}

impl Eq for Peer {}

impl Hash for Peer {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.addr.hash(state)
    }
}

pub fn generate_peer_id() -> PeerId {
    let mut buf = *b"-UT3100-000000000000";
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .zip(&mut buf[8..])
        .for_each(|(c, b)| *b = c as u8);
    buf
}
