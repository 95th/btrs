use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// The string is not a full Bencode packet, more bytes expected
    Eof,

    /// Unexpected character at given position
    Unexpected { pos: usize },

    /// Invalid data at given position
    Invalid { reason: &'static str, pos: usize },

    /// Exceeded Token limit
    TokenLimit { limit: usize },

    /// Exceeded Depth limit
    DepthLimit { limit: usize },

    /// Integer Overflow
    Overflow { pos: usize },

    /// Type mismatch
    TypeMismatch(&'static str),

    /// Other
    Other(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Eof => write!(f, "Unexpected End of File"),
            Self::Unexpected { pos } => write!(f, "Unexpected character at {}", pos),
            Self::Invalid { reason, pos } => write!(f, "Invalid input at {}: {}", pos, reason),
            Self::TokenLimit { limit } => write!(f, "Exceeded Token limit of {}", limit),
            Self::DepthLimit { limit } => write!(f, "Exceeded Depth limit of {}", limit),
            Self::Overflow { pos } => write!(f, "Integer overflow at {}", pos),
            Self::TypeMismatch(reason) => write!(f, "Type mismatch: {}", reason),
            Self::Other(reason) => f.write_str(reason),
        }
    }
}

impl std::error::Error for Error {}
