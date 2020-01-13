use std::borrow::Cow;
use std::fmt;
use std::io;

#[derive(Debug)]
pub struct Error {
    err: Cow<'static, str>,
}

pub type Result<T> = std::result::Result<T, Error>;

impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Error: {}", self.err)
    }
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Self {
            err: e.to_string().into(),
        }
    }
}

impl From<tokio::time::Elapsed> for Error {
    fn from(e: tokio::time::Elapsed) -> Self {
        Self {
            err: e.to_string().into(),
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self {
            err: e.to_string().into(),
        }
    }
}

impl From<&'static str> for Error {
    fn from(err: &'static str) -> Self {
        Self { err: err.into() }
    }
}

impl From<String> for Error {
    fn from(err: String) -> Self {
        Self { err: err.into() }
    }
}
