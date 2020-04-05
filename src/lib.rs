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

#[macro_use]
extern crate anyhow;

#[macro_use]
extern crate log;

pub const CLIENT_NAME: &str = "95th 0.1";

pub mod announce;
pub mod avg;
pub mod bitfield;
pub mod cache;
pub mod client;
pub mod fs;
pub mod future;
pub mod magnet;
pub mod metainfo;
pub mod msg;
pub mod peer;
pub mod torrent;
mod util;
pub mod work;

pub use anyhow::{Error, Result};
