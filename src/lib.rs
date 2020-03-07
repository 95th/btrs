// macro_rules! matches {
//     ($expression:expr, $( $pattern:pat )|+ $( if $guard: expr )?) => {
//         match $expression {
//             $( $pattern )|+ $( if $guard )? => true,
//             _ => false
//         }
//     }
// }

// macro_rules! if_matches {
//     ($expression:expr, $( $pattern:pat )|+ $( if $guard: expr )?, $bind: expr) => {
//         match $expression {
//             $( $pattern )|+ $( if $guard )? => Some($bind),
//             _ => None
//         }
//     }
// }

pub const CLIENT_NAME: &str = "95th 0.1";

pub mod announce;
pub mod avg;
pub mod bitfield;
pub mod cache;
pub mod client;
mod error;
pub mod fs;
pub mod future;
pub mod magnet;
pub mod metainfo;
pub mod msg;
pub mod peer;
pub mod torrent;
pub mod work;

pub use error::*;
