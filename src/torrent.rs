use crate::metainfo::InfoHash;
use sha1::Sha1;

pub const HASH_LEN: usize = 20;

#[derive(Debug)]
pub struct TorrentFile {
    pub announce: String,
    pub info_hash: InfoHash,
    pub pieces: Vec<u8>,
    pub piece_len: usize,
    pub length: usize,
    pub name: String,
}

impl TorrentFile {
    pub fn parse(bytes: &[u8]) -> Option<TorrentFile> {
        let value = bencode::ValueRef::decode(bytes).ok()?;
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
            pieces: pieces.to_vec(),
            piece_len: piece_len as usize,
            length: length as usize,
            name: name.to_owned(),
        };

        Some(torrent)
    }

    pub fn piece_hash(&self, piece_idx: usize) -> &[u8] {
        let start = piece_idx * HASH_LEN;
        let end = self.pieces.len().min(start + HASH_LEN);
        &self.pieces[start..end]
    }
}
