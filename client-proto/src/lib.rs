pub type InfoHash = [u8; 20];
pub type PeerId = [u8; 20];
pub type Extensions = [u8; 8];

pub mod bitfield;
pub mod conn;
pub mod ext;
pub mod handshake;
pub mod msg;
