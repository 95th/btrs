#[macro_use]
extern crate tracing;

#[macro_use]
extern crate anyhow;

pub type InfoHash = [u8; 20];
pub type PeerId = [u8; 20];
pub type Extensions = [u8; 8];

pub mod avg;
pub mod bitfield;
pub mod buf;
pub mod conn;
pub mod event;
mod ext;
mod handshake;
pub mod magnet;
pub mod metainfo;
pub mod msg;
mod state;
pub mod torrent;
