use std::collections::HashSet;
use std::net::SocketAddr;

use crate::metainfo::ParseError;
use anyhow::Context;
use ben::{decode::Dict, Parser};
use sha1::Sha1;

use crate::{magnet::TorrentMagnet, InfoHash};

pub struct Torrent {
    pub info_hash: InfoHash,
    pub piece_hashes: Vec<u8>,
    pub piece_len: usize,
    pub length: usize,
    pub name: String,
    pub tracker_urls: Vec<String>,
    pub peers: HashSet<SocketAddr>,
    pub peers_v6: HashSet<SocketAddr>,
}

impl Torrent {
    pub fn parse_file(data: &[u8]) -> anyhow::Result<Self> {
        use ParseError::*;

        let parser = &mut Parser::new();
        let dict = parser.parse::<Dict>(data)?;
        let announce = dict.get_str("announce").context(AnnounceRequired)?;
        let info = dict.get_dict("info").context(InfoDictRequired)?;
        let info_bytes = info.as_raw_bytes();
        let info_hash = Sha1::from(info_bytes).digest().bytes();

        let length = info.get_int("length").context(LengthRequired)?;
        let name = info.get_str("name").unwrap_or_default();
        let piece_len = info.get_int("piece length").context(PieceLengthRequired)?;
        let pieces = info.get_bytes("pieces").context(PiecesRequired)?;

        let mut tracker_urls = Vec::new();
        tracker_urls.push(announce.to_string());

        if let Some(list) = dict.get_list("announce-list") {
            tracker_urls.extend(
                list.iter()
                    .filter_map(|urls| urls.as_list())
                    .flatten()
                    .filter_map(|url| url.as_str().map(String::from)),
            );
        }

        Ok(Torrent {
            info_hash,
            piece_hashes: pieces.to_vec(),
            piece_len,
            length,
            name: name.to_owned(),
            tracker_urls,
            peers: HashSet::new(),
            peers_v6: HashSet::new(),
        })
    }

    pub fn parse_metainfo(
        magnet: TorrentMagnet,
        info_data: &[u8],
        parser: &mut Parser,
    ) -> anyhow::Result<Self> {
        use ParseError::*;
        let info = parser.parse::<Dict>(info_data)?;

        let length = info.get_int("length").context(LengthRequired)?;
        let name = info
            .get_str("name")
            .map(|s| s.to_string())
            .or(magnet.display_name)
            .unwrap_or_default();
        let piece_len = info.get_int("piece length").context(PieceLengthRequired)?;
        let pieces = info.get_bytes("pieces").context(PiecesRequired)?;

        Ok(Torrent {
            info_hash: magnet.info_hash,
            piece_hashes: pieces.to_vec(),
            piece_len,
            length,
            name,
            tracker_urls: magnet.tracker_urls,
            peers: magnet.peer_addrs,
            peers_v6: HashSet::new(),
        })
    }
}
