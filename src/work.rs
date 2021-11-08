use futures::channel::oneshot;
use rayon::ThreadPool;
use rayon::ThreadPoolBuilder;
use sha1::Sha1;
use std::cell::Cell;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::VecDeque;

use crate::torrent::Torrent;

pub struct WorkQueue {
    pieces: RefCell<VecDeque<PieceInfo>>,
    verifier: PieceVerifier,
    downloaded: Cell<usize>,
}

impl WorkQueue {
    pub fn new(torrent: &Torrent) -> Self {
        let piece_iter = PieceIter::new(torrent.piece_len, torrent.length);

        Self {
            pieces: RefCell::new(piece_iter.collect()),
            downloaded: Cell::new(0),
            verifier: PieceVerifier::new(1, torrent.piece_hashes.clone()),
        }
    }

    pub fn add_piece(&self, info: PieceInfo) {
        self.pieces.borrow_mut().push_back(info);
    }

    pub fn remove_piece(&self) -> Option<PieceInfo> {
        self.pieces.borrow_mut().pop_front()
    }

    pub fn len(&self) -> usize {
        self.pieces.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.pieces.borrow().is_empty()
    }

    pub fn extend<I>(&self, iter: I)
    where
        I: IntoIterator<Item = PieceInfo>,
    {
        self.pieces.borrow_mut().extend(iter);
    }

    pub async fn verify(&self, piece_info: &PieceInfo, data: &[u8]) -> bool {
        self.verifier.verify(piece_info.index as usize, data).await
    }

    pub fn add_downloaded(&self, n: usize) {
        let old = self.downloaded.get();
        self.downloaded.set(old + n);
    }

    pub fn get_downloaded_and_reset(&self) -> usize {
        let n = self.downloaded.get();
        self.downloaded.set(0);
        n
    }
}

#[derive(Debug)]
pub struct PieceInfo {
    pub index: u32,
    pub len: u32,
}

pub struct PieceVerifier {
    pool: ThreadPool,
    hashes: Vec<u8>,
}

impl PieceVerifier {
    pub fn new(num_threads: usize, hashes: Vec<u8>) -> Self {
        Self {
            pool: ThreadPoolBuilder::new()
                .num_threads(num_threads)
                .build()
                .unwrap(),
            hashes,
        }
    }

    async fn verify(&self, index: usize, data: &[u8]) -> bool {
        let expected_hash = &self.hashes[20 * index..][..20];
        let (sender, receiver) = oneshot::channel();

        self.pool.install(|| {
            let actual_hash = Sha1::from(data).digest().bytes();
            let matched = expected_hash == actual_hash;
            let _ = sender.send(matched);
        });

        receiver.await.unwrap()
    }
}

pub struct Piece {
    pub index: u32,
    pub buf: Box<[u8]>,
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

pub struct PieceIter {
    piece_len: u32,
    length: u32,
    index: u32,
}

impl PieceIter {
    pub fn new(piece_len: usize, length: usize) -> Self {
        Self {
            piece_len: piece_len as u32,
            length: length as u32,
            index: 0,
        }
    }
}

impl Iterator for PieceIter {
    type Item = PieceInfo;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index * self.piece_len >= self.length {
            return None;
        }

        let start = self.index * self.piece_len;
        let len = self.piece_len.min(self.length - start);

        let piece = PieceInfo {
            index: self.index,
            len,
        };

        self.index += 1;

        Some(piece)
    }
}
