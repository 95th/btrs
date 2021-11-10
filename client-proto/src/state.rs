use thiserror::Error;

#[derive(Debug, PartialEq)]
pub enum State {
    HandshakeRequired,
    HandshakeSent,
    Ready,
}

#[derive(Debug, PartialEq)]
pub struct MetadataState {
    pub ext_id: u8,
    pub requested_piece: u32,
    pub len: usize,
    pub buf: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Invalid state")]
    InvalidState,

    #[error("Unsupported protocol")]
    UnsupportedProtocol,
}
