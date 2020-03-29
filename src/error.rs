use futures::channel::mpsc::SendError;
use std::borrow::Cow;
use std::fmt;
use std::io;
use tokio::time::Elapsed;

#[derive(Debug)]
pub enum Error {
    Generic(Cow<'static, str>),
    Io(io::Error),
    Bencode(ben::Error),
    Reqwest(reqwest::Error),
    Timer(Elapsed),
    Channel(SendError),
}

pub type Result<T> = std::result::Result<T, Error>;

impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Self::Reqwest(e)
    }
}

impl From<Elapsed> for Error {
    fn from(e: Elapsed) -> Self {
        Self::Timer(e)
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<&'static str> for Error {
    fn from(err: &'static str) -> Self {
        Self::Generic(err.into())
    }
}

impl From<String> for Error {
    fn from(err: String) -> Self {
        Self::Generic(err.into())
    }
}

impl From<SendError> for Error {
    fn from(e: SendError) -> Self {
        Self::Channel(e)
    }
}

impl From<ben::Error> for Error {
    fn from(e: ben::Error) -> Self {
        Self::Bencode(e)
    }
}
