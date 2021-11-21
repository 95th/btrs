use std::{collections::HashSet, net::SocketAddr};

use url::Url;

use crate::{metainfo::MetaInfo, torrent::Torrent, InfoHash};

const SCHEME: &str = "magnet";
const INFOHASH_PREFIX: &str = "urn:btih:";

const TORRENT_ID: &str = "xt";
const DISPLAY_NAME: &str = "dn";
const TRACKER_URL: &str = "tr";
const PEER: &str = "x.pe";

pub struct TorrentMagnet {
    pub info_hash: InfoHash,
    pub display_name: Option<String>,
    pub tracker_urls: Vec<String>,
    pub peer_addrs: HashSet<SocketAddr>,
}

impl TorrentMagnet {
    pub fn parse(uri: &str) -> anyhow::Result<Self> {
        let url = Url::parse(uri).unwrap();
        anyhow::ensure!(url.scheme() == SCHEME, "Incorrect scheme");

        let mut magnet = TorrentMagnet {
            info_hash: InfoHash::default(),
            display_name: None,
            tracker_urls: Vec::new(),
            peer_addrs: HashSet::new(),
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
                    magnet.tracker_urls.push(value.to_string());
                }
                PEER => {
                    if let Ok(addr) = value.parse() {
                        magnet.peer_addrs.insert(addr);
                    }
                }
                _ => {}
            }
        }

        anyhow::ensure!(has_ih, "No infohash found");
        Ok(magnet)
    }

    pub fn with_metadata(self, metadata: MetaInfo) -> Torrent {
        Torrent {
            info_hash: self.info_hash,
            length: metadata.length,
            name: metadata.name.or(self.display_name).unwrap_or_default(),
            piece_hashes: metadata.pieces,
            piece_len: metadata.piece_len,
            tracker_urls: self.tracker_urls,
            peers: HashSet::new(),
            peers_v6: HashSet::new(),
        }
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
