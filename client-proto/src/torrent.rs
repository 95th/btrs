use std::collections::HashSet;

use anyhow::Context;
use ben::{decode::Dict, Parser};
use sha1::Sha1;
use thiserror::Error;

use crate::InfoHash;

pub struct Torrent {
    pub info_hash: InfoHash,
    pub piece_hashes: Vec<u8>,
    pub piece_len: usize,
    pub length: usize,
    pub name: String,
    pub tracker_urls: HashSet<String>,
}

impl Torrent {
    pub fn parse_file(data: &[u8], parser: &mut Parser) -> anyhow::Result<Self> {
        use ParseError::*;

        let dict = parser.parse::<Dict>(data)?;
        let announce = dict.get_str("announce").context(AnnounceRequired)?;
        let info = dict.get_dict("info").context(InfoDictRequired)?;
        let info_bytes = info.as_raw_bytes();
        let info_hash = Sha1::from(info_bytes).digest().bytes().into();

        let length = info.get_int("length").context(LengthRequired)?;
        let name = info.get_str("name").unwrap_or_default();
        let piece_len = info.get_int("piece length").context(PieceLengthRequired)?;
        let pieces = info.get_bytes("pieces").context(PiecesRequired)?;

        let mut tracker_urls = HashSet::new();
        tracker_urls.insert(announce.to_string());

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
            piece_len: piece_len as usize,
            length: length as usize,
            name: name.to_owned(),
            tracker_urls,
        })
    }

    pub fn parse_metainfo(
        data: &[u8],
        info_hash: InfoHash,
        parser: &mut Parser,
    ) -> anyhow::Result<Self> {
        use ParseError::*;
        let info = parser.parse::<Dict>(data)?;

        let length = info.get_int("length").context(LengthRequired)?;
        let name = info.get_str("name").unwrap_or_default();
        let piece_len = info.get_int("piece length").context(PieceLengthRequired)?;
        let pieces = info.get_bytes("pieces").context(PiecesRequired)?;

        Ok(Torrent {
            info_hash,
            piece_hashes: pieces.to_vec(),
            piece_len: piece_len as usize,
            length: length as usize,
            name: name.to_owned(),
            tracker_urls: HashSet::new(),
        })
    }
}

#[derive(Error, Debug)]
enum ParseError {
    #[error("Torrent Piece hash is required")]
    PiecesRequired,

    #[error("Torrent Piece length is required")]
    PieceLengthRequired,

    #[error("Torrent Info dictionary is required")]
    InfoDictRequired,

    #[error("Torrent length is required")]
    LengthRequired,

    #[error("Announce URL is required")]
    AnnounceRequired,
}
