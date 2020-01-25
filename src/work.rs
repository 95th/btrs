use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct PieceWork {
    pub idx: usize,
    pub len: usize,
    pub hash: [u8; 20],
}

pub type WorkQueue = Arc<Mutex<VecDeque<PieceWork>>>;

pub struct PieceResult {
    pub index: usize,
    pub buf: Vec<u8>,
}
