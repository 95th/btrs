use crate::metainfo::InfoHash;
use crate::peer::Peer;
use crate::work::{PieceResult, PieceWork, WorkQueue};
use sha1::Sha1;
use std::convert::TryInto;
use std::ops::Range;
use tokio::sync::mpsc::Sender;

pub const HASH_LEN: usize = 20;

#[derive(Debug)]
pub struct TorrentFile {
    pub announce: String,
    pub info_hash: InfoHash,
    pub piece_hashes: Vec<u8>,
    pub piece_len: usize,
    pub length: usize,
    pub name: String,
}

impl TorrentFile {
    pub fn parse(bytes: impl AsRef<[u8]>) -> Option<TorrentFile> {
        let value = bencode::ValueRef::decode(bytes.as_ref()).ok()?;
        let dict = value.as_dict()?;
        let announce = dict.get("announce")?.as_str()?;
        let info_dict = dict.get("info")?.as_dict()?;
        let info_bytes = dict.get("info")?.encode_to_vec();
        let info_hash = Sha1::from(info_bytes).digest().bytes().into();

        let length = info_dict.get("length")?.as_int()?;
        let name = info_dict
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or_default();
        let piece_len = info_dict.get("piece length")?.as_int()?;
        let pieces = info_dict.get("pieces")?.as_bytes()?;

        let torrent = TorrentFile {
            announce: announce.to_owned(),
            info_hash,
            piece_hashes: pieces.to_vec(),
            piece_len: piece_len as usize,
            length: length as usize,
            name: name.to_owned(),
        };

        Some(torrent)
    }

    pub fn piece_iter(&self) -> PieceIter {
        PieceIter::new(self)
    }

    pub fn piece_bounds(&self, idx: usize) -> Range<usize> {
        let start = idx * self.piece_len;
        let end = start + self.piece_len;
        start..end.min(self.length)
    }

    pub async fn start_worker(
        &self,
        _peer: Peer,
        _work_queue: WorkQueue,
        _result_tx: Sender<PieceResult>,
    ) {
        todo!()
    }
}

pub struct PieceIter<'a> {
    torrent: &'a TorrentFile,
    idx: usize,
    count: usize,
}

impl<'a> PieceIter<'a> {
    fn new(torrent: &'a TorrentFile) -> Self {
        Self {
            torrent,
            idx: 0,
            count: torrent.piece_hashes.len() / 20,
        }
    }
}

impl Iterator for PieceIter<'_> {
    type Item = PieceWork;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx >= self.count {
            return None;
        }

        let bytes = &self.torrent.piece_hashes[self.idx * HASH_LEN..][..HASH_LEN];
        let hash = bytes.try_into().unwrap();

        let start = self.idx * self.torrent.piece_len;
        let end = start + self.torrent.piece_len;

        let piece = PieceWork {
            idx: self.idx,
            len: end.min(self.torrent.length) - start,
            hash,
        };

        self.idx += 1;

        Some(piece)
    }
}
