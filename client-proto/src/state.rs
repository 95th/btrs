use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Unsupported protocol")]
    UnsupportedProtocol,
}
