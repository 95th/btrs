//! `ben` is an efficient Bencode parser which parses the structure into
//! a flat stream of tokens rather than an actual tree and thus avoids
//! unneccessary allocations.

pub mod decode;
mod encode;
mod error;
mod parse;
mod token;

pub use decode::{Decode, Decoder};
pub use encode::{
    write_bytes, write_int, DictEncoder, Encode, ExactBytesEncoder, ListEncoder, SortedDictEncoder,
};
pub use error::{Error, Result};
pub use parse::Parser;
