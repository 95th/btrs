use sha1::Sha1;
use std::cell::RefCell;
use std::collections::VecDeque;

pub type WorkQueue<'a> = RefCell<VecDeque<PieceWork<'a>>>;

pub struct PieceWork<'a> {
    pub index: usize,
    pub len: usize,
    pub hash: &'a [u8],
}

impl PieceWork<'_> {
    pub fn check_integrity(&self, buf: &[u8]) -> bool {
        let hash = Sha1::from(buf).digest().bytes();
        hash == self.hash
    }
}

pub struct Piece {
    pub index: usize,
    pub buf: Vec<u8>,
}
