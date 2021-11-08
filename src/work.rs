use futures::channel::oneshot;
use rayon::ThreadPool;
use rayon::ThreadPoolBuilder;
use sha1::Sha1;
use std::cell::Cell;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::VecDeque;
use std::slice::Chunks;

use crate::torrent::Torrent;

pub struct WorkQueue {
    pieces: RefCell<VecDeque<PieceInfo>>,
    verifier: PieceVerifier,
    downloaded: Cell<usize>,
}

impl WorkQueue {
    pub fn new(torrent: &Torrent) -> Self {
        let piece_iter = PieceIter::new(
            &torrent.piece_hashes,
            HashKind::Sha1,
            torrent.piece_len,
            torrent.length,
        );

        Self {
            pieces: RefCell::new(piece_iter.collect()),
            downloaded: Cell::new(0),
            verifier: PieceVerifier::new(1),
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
        self.verifier.verify(piece_info, data).await
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
    pub hash: Vec<u8>,
}

pub struct PieceVerifier {
    pool: ThreadPool,
}

impl PieceVerifier {
    pub fn new(num_threads: usize) -> Self {
        Self {
            pool: ThreadPoolBuilder::new()
                .num_threads(num_threads)
                .build()
                .unwrap(),
        }
    }

    async fn verify(&self, piece_info: &PieceInfo, data: &[u8]) -> bool {
        let (tx, rx) = oneshot::channel();
        self.pool.install(|| {
            let actual_hash = Sha1::from(data).digest().bytes();
            let ok = piece_info.hash == actual_hash;
            let _ = tx.send(ok);
        });
        rx.await.unwrap()
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
            hash: hash.to_vec(),
        };

        self.index += 1;

        Some(piece)
    }
}
