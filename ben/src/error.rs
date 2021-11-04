use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error, Clone, PartialEq)]
pub enum Error {
    /// The string is not a full Bencode packet, more bytes expected
    #[error("Unexpected End of File")]
    Eof,

    /// Unexpected character at given position
    #[error("Unexpected character at {pos}")]
    Unexpected { pos: usize },

    /// Invalid data at given position
    #[error("Invalid input at {reason}: {reason}")]
    Invalid { reason: &'static str, pos: usize },

    /// Exceeded Token limit
    #[error("Exceeded Token limit of {limit}")]
    TokenLimit { limit: usize },

    /// Exceeded Depth limit
    #[error("Exceeded Depth limit of {limit}")]
    DepthLimit { limit: usize },

    /// Integer Overflow
    #[error("Integer overflow at {pos}")]
    Overflow { pos: usize },

    /// Type mismatch
    #[error("Type mismatch: {0}")]
    TypeMismatch(&'static str),

    /// Other
    #[error("{0}")]
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
