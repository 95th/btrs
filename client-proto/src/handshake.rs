use crate::{Extensions, InfoHash, PeerId};

const PROTOCOL: &[u8; 20] = b"\x13BitTorrent protocol";

#[derive(Debug, Default)]
#[repr(C)]
pub struct Handshake {
    protocol: [u8; 20],
    extensions: Extensions,
    pub info_hash: InfoHash,
    pub peer_id: PeerId,
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

    pub fn as_bytes(&self) -> &[u8; 68] {
        let ptr = self as *const Handshake;
        unsafe { &*ptr.cast() }
    }

    pub fn is_supported(&self) -> bool {
        self.protocol == *PROTOCOL
    }
}
