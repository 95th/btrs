use crate::metainfo::InfoHash;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct PieceWork {
    pub index: usize,
    pub len: usize,
    pub hash: InfoHash,
}

pub type WorkQueue = Arc<Mutex<VecDeque<PieceWork>>>;

pub struct PieceResult {
    pub index: usize,
    pub buf: Vec<u8>,
}
