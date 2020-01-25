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

pub const CLIENT_VERSION: &str = "950001";

pub mod bitfield;
pub mod client;
pub mod conn;
mod error;
pub mod future;
pub mod magnet;
pub mod metainfo;
pub mod peer;
pub mod torrent;

pub use error::*;
