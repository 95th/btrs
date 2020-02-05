use crate::metainfo::InfoHash;
use sha1::Sha1;
use std::collections::VecDeque;
use tokio::sync::Mutex;

pub type WorkQueue = Mutex<VecDeque<PieceWork>>;

pub struct PieceWork {
    pub index: usize,
    pub len: usize,
    pub hash: InfoHash,
}

impl PieceWork {
    pub fn check_integrity(&self, buf: &[u8]) -> bool {
        let hash = Sha1::from(buf).digest().bytes();
        hash == *self.hash.as_ref()
    }
}

pub struct Piece {
    pub index: usize,
    pub buf: Vec<u8>,
}
