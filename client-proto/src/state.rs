use thiserror::Error;

#[derive(Debug, PartialEq)]
pub enum State {
    HandshakeRequired,
    HandshakeSent,
    Ready,
    MetadataRequested(MetadataState),
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

    #[error("Invalid handshake data")]
    InvalidHandshake,

    #[error("Packet too large")]
    PacketTooLarge,
}
