use futures::channel::oneshot;
use rayon::ThreadPool;
use rayon::ThreadPoolBuilder;
use sha1::Sha1;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::VecDeque;
use std::rc::Rc;
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
            let actual_hash =  Sha1::from(&data).digest().bytes();
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

pub struct PieceIter<'a, const N: usize> {
    piece_hashes: &'a [u8],
    piece_len: usize,
    length: usize,
    index: u32,
    count: u32,
}

impl<'a, const N: usize> PieceIter<'a, N> {
    pub fn new(piece_hashes: &'a [u8], piece_len: usize, length: usize) -> Self {
        Self {
            piece_hashes,
            piece_len,
            length,
            index: 0,
            count: (piece_hashes.len() / 20) as u32,
        }
    }
}

impl<'a, const N: usize> Iterator for PieceIter<'a, N> {
    type Item = PieceInfo;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.count {
            return None;
        }

        let hash = self.piece_hashes[self.index as usize * N..][..N].into();

        let piece_len = self.piece_len as u32;
        let start = self.index * piece_len;
        let len = piece_len.min(self.length as u32 - start);

        let piece = PieceInfo {
            index: self.index,
            len,
            hash,
        };

        self.index += 1;

        Some(piece)
    }
}
