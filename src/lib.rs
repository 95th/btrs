#[macro_use]
extern crate tracing;

macro_rules! hashset {
    () => {
        std::collections::HashSet::new()
    };
    ($($x:expr),*) => {{
        let mut set = hashset![];
        $(
            set.insert($x);
        )*
        set
    }};
    ($($x:expr,)*) => (hashset![$($x),*])
}

pub const CLIENT_NAME: &str = "95th 0.1";

pub mod announce;
mod download;
pub mod future;
pub mod metadata;
pub mod peer;
pub mod storage;
pub mod work;
mod worker;

pub use client::torrent::*;
pub use worker::TorrentWorker;
