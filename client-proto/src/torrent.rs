use std::{collections::HashSet, net::SocketAddr};

use anyhow::{bail, ensure, Context};
use ben::{decode::Dict, Parser};
use sha1::Sha1;
use thiserror::Error;
use url::Url;

use crate::InfoHash;

pub struct Torrent {
    pub info_hash: InfoHash,
    pub piece_hashes: Vec<u8>,
    pub piece_len: usize,
    pub length: usize,
    pub name: String,
    pub tracker_urls: HashSet<String>,
    pub peers: Vec<SocketAddr>,
    pub peers_v6: Vec<SocketAddr>,
}

impl Torrent {
    pub fn parse_file(data: &[u8], parser: &mut Parser) -> anyhow::Result<Self> {
        use ParseError::*;

        let dict = parser.parse::<Dict>(data)?;
        let announce = dict.get_str("announce").context(AnnounceRequired)?;
        let info = dict.get_dict("info").context(InfoDictRequired)?;
        let info_bytes = info.as_raw_bytes();
        let info_hash = Sha1::from(info_bytes).digest().bytes();

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
            peers: Vec::new(),
            peers_v6: Vec::new(),
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
            piece_len: piece_len as usize,
            length: length as usize,
            name,
            tracker_urls: magnet.tracker_urls,
            peers: magnet.peer_addrs,
            peers_v6: Vec::new(),
        })
    }
}

const SCHEME: &str = "magnet";
const INFOHASH_PREFIX: &str = "urn:btih:";

const TORRENT_ID: &str = "xt";
const DISPLAY_NAME: &str = "dn";
const TRACKER_URL: &str = "tr";
const PEER: &str = "x.pe";

pub struct TorrentMagnet {
    info_hash: InfoHash,
    display_name: Option<String>,
    tracker_urls: HashSet<String>,
    peer_addrs: Vec<SocketAddr>,
}

impl TorrentMagnet {
    pub fn parse(uri: &str) -> anyhow::Result<Self> {
        let url = Url::parse(uri).unwrap();
        anyhow::ensure!(url.scheme() == SCHEME, "Incorrect scheme");

        let mut magnet = TorrentMagnet {
            info_hash: InfoHash::default(),
            display_name: None,
            tracker_urls: HashSet::new(),
            peer_addrs: Vec::new(),
        };

        let mut has_ih = false;

        for (key, value) in url.query_pairs() {
            match &key[..] {
                TORRENT_ID => {
                    if let Some(ih_str) = value.strip_prefix(INFOHASH_PREFIX) {
                        ensure!(!has_ih, "Multiple infohashes found");
                        decode_infohash(ih_str, &mut magnet.info_hash)?;
                        has_ih = true;
                    }
                }
                DISPLAY_NAME => magnet.display_name = Some(value.to_string()),
                TRACKER_URL => {
                    magnet.tracker_urls.insert(value.to_string());
                }
                PEER => {
                    if let Ok(addr) = value.parse() {
                        magnet.peer_addrs.push(addr);
                    }
                }
                _ => {}
            }
        }

        anyhow::ensure!(has_ih, "No infohash found");
        Ok(magnet)
    }
}

fn decode_infohash(encoded: &str, info_hash: &mut InfoHash) -> anyhow::Result<()> {
    use data_encoding::{BASE32, HEXLOWER_PERMISSIVE as HEX};

    let encoded = encoded.as_bytes();

    let result = match encoded.len() {
        40 => HEX.decode_mut(encoded, info_hash),
        32 => BASE32.decode_mut(encoded, info_hash),
        _ => bail!("Invalid infohash length"),
    };

    ensure!(result.is_ok(), "Invalid infohash");
    Ok(())
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
