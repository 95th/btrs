use futures::channel::oneshot;
use rayon::ThreadPool;
use rayon::ThreadPoolBuilder;
use sha1::Sha1;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::VecDeque;
use std::rc::Rc;
use std::slice::Chunks;
use std::sync::Arc;

pub type WorkQueue = RefCell<VecDeque<PieceInfo>>;

#[derive(Debug)]
pub struct PieceInfo {
    pub index: u32,
    pub len: u32,
    pub hash: Arc<[u8]>,
}

#[derive(Clone)]
pub struct PieceVerifier {
    pool: Rc<ThreadPool>,
}

impl PieceVerifier {
    pub fn new(num_threads: usize) -> Self {
        Self {
            pool: Rc::new(
                ThreadPoolBuilder::new()
                    .num_threads(num_threads)
                    .build()
                    .unwrap(),
            ),
        }
    }

    pub async fn verify(&self, piece_info: &PieceInfo, data: &Arc<[u8]>) -> bool {
        let (tx, rx) = oneshot::channel();
        let expected_hash = piece_info.hash.clone();
        let data = data.clone();
        self.pool.spawn(move || {
            let actual_hash = Sha1::from(&data).digest().bytes();
            let ok = expected_hash[..] == actual_hash;
            tx.send(ok).unwrap();
        });
        rx.await.unwrap()
    }
}

pub struct Piece {
    pub index: u32,
    pub buf: Arc<[u8]>,
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

pub enum HashKind {
    Sha1 = 20,
}

pub struct PieceIter<'a> {
    chunks: Chunks<'a, u8>,
    piece_len: u32,
    length: u32,
    index: u32,
}

impl<'a> PieceIter<'a> {
    pub fn new(
        piece_hashes: &'a [u8],
        hash_kind: HashKind,
        piece_len: usize,
        length: usize,
    ) -> Self {
        let chunks = piece_hashes.chunks(hash_kind as usize);
        Self {
            chunks,
            piece_len: piece_len as u32,
            length: length as u32,
            index: 0,
        }
    }
}

impl<'a> Iterator for PieceIter<'a> {
    type Item = PieceInfo;

    fn next(&mut self) -> Option<Self::Item> {
        let hash = self.chunks.next()?;
        let piece_len = self.piece_len;
        let start = self.index * piece_len;
        let len = piece_len.min(self.length - start);

        let piece = PieceInfo {
            index: self.index,
            len,
            hash: hash.into(),
        };

        self.index += 1;

        Some(piece)
    }
}
