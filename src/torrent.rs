use crate::{
    announce::DhtTracker,
    metainfo::InfoHash,
    peer::{self, Peer, PeerId},
    worker::TorrentWorker,
};
use anyhow::Context;
use ben::{decode::Dict, Parser};
use sha1::Sha1;
use std::{collections::HashSet, fmt};

pub struct TorrentFile {
    pub tracker_urls: HashSet<String>,
    pub info_hash: InfoHash,
    pub piece_hashes: Vec<u8>,
    pub piece_len: usize,
    pub length: usize,
    pub name: String,
}

impl fmt::Debug for TorrentFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TorrentFile")
            .field("tracker_urls", &self.tracker_urls)
            .field("info_hash", &self.info_hash)
            .field(
                "piece_hashes",
                &format!("[..; {}]", self.piece_hashes.len()),
            )
            .field("piece_len", &self.piece_len)
            .field("length", &self.length)
            .field("name", &self.name)
            .finish()
    }
}

impl TorrentFile {
    pub fn parse(bytes: impl AsRef<[u8]>) -> anyhow::Result<TorrentFile> {
        let mut parser = Parser::new();
        let dict = parser.parse::<Dict>(bytes.as_ref())?;
        let announce = dict.get_str("announce").context("`announce` not found")?;
        let info = dict.get_dict("info").context("`info` dict not found")?;
        let info_bytes = info.as_raw_bytes();
        let info_hash = Sha1::from(info_bytes).digest().bytes().into();

        let length = info.get_int("length").context("`length` not found")?;
        let name = info.get_str("name").unwrap_or_default();
        let piece_len = info
            .get_int("piece length")
            .context("`piece length` not found")?;
        let pieces = info.get_bytes("pieces").context("`pieces` not found")?;

        let mut tracker_urls = hashset![announce.to_owned()];
        if let Some(list) = dict.get_list("announce-list") {
            for v in list.iter() {
                for v in v.into_list().context("`announce-list` is not a list")?.iter() {
                    tracker_urls.insert(
                        v.as_str()
                            .context("URL in `announce-list` is not a valid string")?
                            .to_string(),
                    );
                }
            }
        }

        let torrent = TorrentFile {
            tracker_urls,
            info_hash,
            piece_hashes: pieces.to_vec(),
            piece_len: piece_len as usize,
            length: length as usize,
            name: name.to_owned(),
        };

        Ok(torrent)
    }

    pub async fn into_torrent(self) -> anyhow::Result<Torrent> {
        let peer_id = peer::generate_peer_id();
        let dht_tracker = DhtTracker::new().await?;

        Ok(Torrent {
            peer_id,
            info_hash: self.info_hash,
            piece_hashes: self.piece_hashes,
            piece_len: self.piece_len,
            length: self.length,
            name: self.name,
            tracker_urls: self.tracker_urls,
            peers: hashset![],
            peers6: hashset![],
            dht_tracker,
        })
    }
}

pub struct Torrent {
    pub peer_id: PeerId,
    pub info_hash: InfoHash,
    pub piece_hashes: Vec<u8>,
    pub piece_len: usize,
    pub length: usize,
    pub name: String,
    pub tracker_urls: HashSet<String>,
    pub peers: HashSet<Peer>,
    pub peers6: HashSet<Peer>,
    pub dht_tracker: DhtTracker,
}

impl Torrent {
    pub fn worker(&mut self) -> TorrentWorker<'_> {
        TorrentWorker::new(self)
    }
}
