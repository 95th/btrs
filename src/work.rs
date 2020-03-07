use sha1::Sha1;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::VecDeque;

pub type WorkQueue<'a> = RefCell<VecDeque<PieceWork<'a>>>;

#[derive(Debug)]
pub struct PieceWork<'a> {
    pub index: u32,
    pub len: u32,
    pub hash: &'a [u8],
}

impl PieceWork<'_> {
    pub fn check_integrity(&self, buf: &[u8]) -> bool {
        let hash = Sha1::from(buf).digest().bytes();
        hash == self.hash
    }
}

pub struct Piece {
    pub index: u32,
    pub buf: Vec<u8>,
}

impl PartialEq for Piece {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}

impl Eq for Piece {}

impl PartialOrd for Piece {
    fn partial_cmp(&self, other: &Piece) -> Option<Ordering> {
        other.index.partial_cmp(&self.index)
    }
}

impl Ord for Piece {
    fn cmp(&self, other: &Piece) -> Ordering {
        other.index.cmp(&self.index)
    }
}
