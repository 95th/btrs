use core::fmt;

use crate::{Extensions, InfoHash, PeerId};

const PROTOCOL: &[u8; 20] = b"\x13BitTorrent protocol";
const HANDSHAKE_LEN: usize = std::mem::size_of::<Handshake>();

#[derive(Debug)]
pub struct Handshake {
    protocol: [u8; 20],
    extensions: Extensions,
    info_hash: InfoHash,
    peer_id: PeerId,
}

impl Handshake {
    pub fn new(info_hash: InfoHash, peer_id: PeerId) -> Self {
        Self::with_extensions(Extensions::default(), info_hash, peer_id)
    }

    pub fn with_extensions(extensions: Extensions, info_hash: InfoHash, peer_id: PeerId) -> Self {
        Self {
            protocol: *PROTOCOL,
            extensions,
            info_hash,
            peer_id,
        }
    }

    pub fn set_extended(&mut self, enable: bool) {
        if enable {
            self.extensions[5] |= 0x10;
        } else {
            self.extensions[5] &= !0x10;
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        let ptr = self as *const Handshake as *const u8;
        unsafe { std::slice::from_raw_parts(ptr, HANDSHAKE_LEN) }
    }

    pub fn read<'a>(&self, buf: &'a [u8]) -> Result<&'a Handshake, Error> {
        log::trace!("Read handshake message");

        if buf.len() != HANDSHAKE_LEN {
            return Err(Error::Invalid);
        }

        let other = unsafe { &*buf.as_ptr().cast::<Handshake>() };
        if self.protocol != other.protocol {
            return Err(Error::Protocol);
        }

        if self.info_hash != other.info_hash {
            return Err(Error::Infohash);
        }

        Ok(other)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Error {
    Invalid,
    Infohash,
    Protocol,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Invalid => f.write_str("Invalid handshake data"),
            Error::Infohash => f.write_str("Infohash mismatch"),
            Error::Protocol => f.write_str("Protocol mismatch"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_align() {
        assert_eq!(std::mem::align_of::<Handshake>(), 1);
    }

    #[test]
    fn handshake_read_ok() {
        let h = Handshake::new([1; 20], [2; 20]);
        let b = Handshake::new([1; 20], [3; 20]);
        h.read(b.as_bytes()).unwrap();
    }

    #[test]
    fn infohash_mismatch() {
        let h = Handshake::new([1; 20], [2; 20]);
        let b = Handshake::new([2; 20], [3; 20]);
        let err = h.read(b.as_bytes()).unwrap_err();
        assert_eq!(err, Error::Infohash);
    }

    #[test]
    fn protocol_mismatch() {
        let h = Handshake::new([1; 20], [2; 20]);
        let b = &[0; 68];
        let err = h.read(b).unwrap_err();
        assert_eq!(err, Error::Protocol);
    }

    #[test]
    fn invalid_length() {
        let h = Handshake::new([1; 20], [2; 20]);
        let b = &[0];
        let err = h.read(b).unwrap_err();
        assert_eq!(err, Error::Invalid);
    }

    #[test]
    fn handshake_bytes() {
        let h = Handshake::new([1; 20], [2; 20]);
        let mut b = [0; 68];
        b[..20].copy_from_slice(PROTOCOL);
        b[28..48].fill(1);
        b[48..68].fill(2);
        assert_eq!(b, h.as_bytes());
    }
}
