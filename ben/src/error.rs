use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Error {
    /// The string is not a full Bencode packet, more bytes expected
    Eof,

    /// Invalid character inside Bencode string
    Unexpected { pos: usize },

    /// Invalid character inside Bencode string
    Invalid { reason: &'static str, pos: usize },

    /// Not enough tokens were provided
    NoMemory,

    /// Integer Overflow
    Overflow { pos: usize },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Eof => write!(f, "Unexpected End of File"),
            Self::Unexpected { pos } => write!(f, "Unexpected character at {}", pos),
            Self::Invalid { reason, pos } => write!(f, "Invalid input at {}: {}", pos, reason),
            Self::NoMemory => write!(f, "No tokens left to parse"),
            Self::Overflow { pos } => write!(f, "Integer overflow at {}", pos),
        }
    }
}

impl std::error::Error for Error {}
