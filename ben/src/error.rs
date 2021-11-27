use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error, Clone, PartialEq)]
pub enum Error {
    #[error("Unexpected End of File")]
    /// The string is not a full Bencode packet, more bytes expected
    Eof,

    #[error("Unexpected trailing data in the input")]
    /// Unexpected trailing data in the input
    TrailingData,

    #[error("Invalid input")]
    /// Invalid data at given position
    Invalid,

    #[error("Exceeded Token limit")]
    /// Exceeded Token limit
    TokenLimit,

    #[error("Exceeded Depth limit")]
    /// Exceeded Depth limit
    DepthLimit,

    #[error("Integer overflow")]
    /// Integer Overflow
    Overflow,

    #[error("Decode error")]
    /// Decode error
    Decode,
}
