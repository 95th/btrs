use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error, Clone, PartialEq)]
pub enum Error {
    #[error("Unexpected End of File")]
    /// The string is not a full Bencode packet, more bytes expected
    Eof,

    #[error("Unexpected character at {pos}")]
    /// Unexpected character at given position
    Unexpected { pos: usize },

    #[error("Invalid input at {reason}: {reason}")]
    /// Invalid data at given position
    Invalid { reason: &'static str, pos: usize },

    #[error("Exceeded Token limit of {limit}")]
    /// Exceeded Token limit
    TokenLimit { limit: usize },

    #[error("Exceeded Depth limit of {limit}")]
    /// Exceeded Depth limit
    DepthLimit { limit: usize },

    #[error("Integer overflow at {pos}")]
    /// Integer Overflow
    Overflow { pos: usize },

    #[error("Unable to decode")]
    /// Decode error
    Decode,

    #[error("{0}")]
    /// Other
    Other(&'static str),
}

impl Error {
    pub fn unexpected(pos: usize) -> Self {
        Self::Unexpected { pos }
    }

    pub fn overflow(pos: usize) -> Self {
        Self::Overflow { pos }
    }
}
