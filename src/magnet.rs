use crate::announce::{DhtTracker, Tracker};
use crate::future::timeout;
use crate::metainfo::InfoHash;
use crate::peer::{Peer, PeerId};
use crate::torrent::Torrent;
use anyhow::Context;
use ben::decode::Dict;
use ben::Parser;
use client::Client;
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use std::collections::HashSet;
use std::net::SocketAddr;
use tokio::net::TcpStream;

#[derive(Debug, Default)]
pub struct MagnetUri {
    info_hash: InfoHash,
    display_name: Option<String>,
    tracker_urls: HashSet<String>,
    peer_addrs: Vec<SocketAddr>,
}

struct TorrentInfo {
    piece_len: usize,
    length: usize,
    piece_hashes: Vec<u8>,
    name: String,
}

impl MagnetUri {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        parser::MagnetUriParser::new().parse(s)
    }

    pub fn parse_lenient(s: &str) -> anyhow::Result<Self> {
        parser::MagnetUriParser::new_lenient().parse(s)
    }

    pub async fn request_metadata(&self, peer_id: PeerId) -> anyhow::Result<Torrent> {
        let (peers, peers6, dht_tracker) = self.get_peers(&peer_id).await?;

        let mut futures = FuturesUnordered::new();
        let mut peers_iter = peers.iter().chain(&peers6);

        loop {
            if futures.len() < 20 {
                while let Some(p) = peers_iter.next() {
                    futures.push(timeout(self.try_get(p, &peer_id), 60));
                }
            }

            if let Some(result) = futures.next().await {
                match result {
                    Ok(data) => {
                        if let Some(t) = self.read_info(&data) {
                            drop(futures);
                            trace!("Metadata requested successfully");
                            return Ok(Torrent {
                                peer_id,
                                info_hash: self.info_hash.clone(),
                                piece_len: t.piece_len,
                                length: t.length,
                                piece_hashes: t.piece_hashes,
                                name: t.name,
                                tracker_urls: self.tracker_urls.clone(),
                                peers,
                                peers6,
                                dht_tracker,
                            });
                        }
                    }
                    Err(e) => debug!("Error : {}", e),
                }
            } else {
                break;
            }
        }

        anyhow::bail!("Metadata request failed")
    }

    fn read_info(&self, data: &[u8]) -> Option<TorrentInfo> {
        trace!("Read torrent info, len: {}", data.len());
        let parser = &mut Parser::new();
        let info_dict = match parser.parse::<Dict>(data) {
            Ok(d) => d,
            Err(e) => {
                warn!("{}", e);
                return None;
            }
        };

        info!("Got dict: {:?}", info_dict);

        let length = info_dict.get_int("length")? as usize;
        let name = info_dict.get_str("name").unwrap_or_default().to_string();
        let piece_len = info_dict.get_int("piece length")? as usize;
        let piece_hashes = info_dict.get_bytes("pieces")?.to_vec();
        Some(TorrentInfo {
            piece_len,
            length,
            piece_hashes,
            name,
        })
    }

    async fn get_peers(
        &self,
        peer_id: &PeerId,
    ) -> anyhow::Result<(HashSet<Peer>, HashSet<Peer>, DhtTracker)> {
        debug!("Requesting peers");

        let mut futs: FuturesUnordered<_> = self
            .tracker_urls
            .iter()
            .map(|url| async move {
                let mut t = Tracker::new(url);
                t.announce(&self.info_hash, peer_id).await
            })
            .collect();

        let mut peers = hashset![];
        let mut peers6 = hashset![];

        while let Some(r) = futs.next().await {
            match r {
                Ok(r) => {
                    peers.extend(r.peers);
                    peers6.extend(r.peers6);
                }
                Err(e) => debug!("Error: {}", e),
            }
        }

        debug!("Got {} v4 peers and {} v6 peers", peers.len(), peers6.len());

        let mut dht_tracker = DhtTracker::new().await?;
        if peers.is_empty() && peers6.is_empty() {
            if let Ok(p) = dht_tracker.announce(&self.info_hash).await {
                peers.extend(p);
            }
            debug!(
                "Got {} v4 peers and {} v6 peers from DHT",
                peers.len(),
                peers6.len()
            );
        }

        if peers.is_empty() && peers6.is_empty() {
            anyhow::bail!("No peers received from trackers");
        }

        Ok((peers, peers6, dht_tracker))
    }

    #[instrument(skip_all, fields(addr = ?peer.addr))]
    async fn try_get(&self, peer: &Peer, peer_id: &PeerId) -> anyhow::Result<Vec<u8>> {
        let socket = TcpStream::connect(peer.addr).await?;
        let mut client = Client::new(socket);
        client.send_handshake(&self.info_hash, peer_id).await?;
        client.recv_handshake(&self.info_hash).await?;
        client.get_metadata().await
    }
}

mod parser {
    use super::*;
    use url::Url;

    pub struct MagnetUriParser {
        strict: bool,
    }

    const SCHEME: &str = "magnet";
    const INFOHASH_PREFIX: &str = "urn:btih:";

    const TORRENT_ID: &str = "xt";
    const DISPLAY_NAME: &str = "dn";
    const TRACKER_URL: &str = "tr";
    const PEER: &str = "x.pe";

    impl MagnetUriParser {
        pub fn new() -> Self {
            Self { strict: true }
        }

        pub fn new_lenient() -> Self {
            Self { strict: false }
        }

        pub fn parse(&self, uri: &str) -> anyhow::Result<MagnetUri> {
            let url = Url::parse(uri).unwrap();
            anyhow::ensure!(url.scheme() == SCHEME, "Incorrect scheme");

            let mut magnet = MagnetUri::default();
            let mut has_ih = false;
            for (key, value) in url.query_pairs() {
                match &key[..] {
                    TORRENT_ID => {
                        if let Some(ih_str) = value.strip_prefix(INFOHASH_PREFIX) {
                            let info_hash = build_info_hash(ih_str)?;

                            if has_ih && info_hash != magnet.info_hash {
                                anyhow::bail!("Multiple infohashes found");
                            }

                            magnet.info_hash = info_hash;
                            has_ih = true;
                        }
                    }
                    DISPLAY_NAME => magnet.display_name = Some(value.to_string()),
                    TRACKER_URL => {
                        magnet.tracker_urls.insert(value.to_string());
                    }
                    PEER => match value.parse() {
                        Ok(addr) => magnet.peer_addrs.push(addr),
                        Err(_) => {
                            if self.strict {
                                anyhow::bail!("Invalid peer addr");
                            }
                        }
                    },
                    _ => {}
                }
            }

            anyhow::ensure!(has_ih, "No infohash found");
            Ok(magnet)
        }
    }

    fn build_info_hash(encoded: &str) -> anyhow::Result<InfoHash> {
        use data_encoding::{BASE32 as base32, HEXLOWER_PERMISSIVE as hex};

        let encoded = encoded.as_bytes();
        let mut id = InfoHash::default();

        match encoded.len() {
            40 => {
                hex.decode_mut(encoded, id.as_mut())
                    .ok()
                    .context("Invalid hex string")?;
            }
            32 => {
                base32
                    .decode_mut(encoded, id.as_mut())
                    .ok()
                    .context("Invalid base 32 string")?;
            }
            _ => anyhow::bail!("Invalid infohash length"),
        }

        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use data_encoding::{BASE32, HEXLOWER_PERMISSIVE};

    use super::*;

    fn encode_hex(infohash: InfoHash) -> String {
        HEXLOWER_PERMISSIVE.encode(&infohash)
    }

    fn encode_base32(infohash: InfoHash) -> String {
        BASE32.encode(&infohash)
    }

    #[test]
    fn parse_hex_infohash() {
        let infohash = InfoHash::from([12; 20]);
        let s = format!("magnet:?xt=urn:btih:{}", encode_hex(infohash));
        let magnet = MagnetUri::parse(&s).unwrap();
        assert_eq!(infohash, magnet.info_hash);
    }

    #[test]
    fn parse_base32_infohash() {
        let infohash = InfoHash::from([12; 20]);
        let s = format!("magnet:?xt=urn:btih:{}", encode_base32(infohash));
        let magnet = MagnetUri::parse(&s).unwrap();
        assert_eq!(infohash, magnet.info_hash);
    }

    #[test]
    fn parse_all_params_present() {
        let infohash = InfoHash::from([0; 20]);
        let display_name = "xyz";
        let tracker_url_1 = "http://jupiter.gx/ann";
        let tracker_url_2 = "udp://jupiter.gx:1111";
        let peer_1 = "1.1.1.1:10000";
        let peer_2 = "2.2.2.2:10000";
        let s = format!(
            "magnet:?xt=urn:btih:{}&dn={}&tr={}&tr={}&x.pe={}&x.pe={}",
            encode_hex(infohash),
            display_name,
            tracker_url_1,
            tracker_url_2,
            peer_1,
            peer_2
        );
        let magnet = MagnetUri::parse(&s).unwrap();
        assert_eq!(infohash, magnet.info_hash);
        assert_eq!(display_name, magnet.display_name.unwrap());

        let urls: HashSet<&str> = magnet.tracker_urls.iter().map(|s| &s[..]).collect();
        assert_eq!(hashset![tracker_url_1, tracker_url_2], urls);

        let peers: &[SocketAddr] = &[peer_1.parse().unwrap(), peer_2.parse().unwrap()];
        assert_eq!(peers, &magnet.peer_addrs[..]);
    }

    #[test]
    fn parse_only_infohash_present() {
        let infohash = InfoHash::from([0; 20]);
        let s = format!("magnet:?xt=urn:btih:{}", encode_hex(infohash));
        let magnet = MagnetUri::parse(&s).unwrap();
        assert_eq!(infohash, magnet.info_hash);
    }

    #[test]
    fn parse_both_infohash_and_multihash_present() {
        let infohash = InfoHash::from([0; 20]);
        let multihash = InfoHash::from([1; 20]);
        let s = format!(
            "magnet:?xt=urn:btih:{}&xt=urn:btmh:{}",
            encode_hex(infohash),
            encode_hex(multihash),
        );
        let magnet = MagnetUri::parse(&s).unwrap();
        assert_eq!(infohash, magnet.info_hash);
    }

    #[test]
    fn parse_multiple_infohashes_present() {
        let infohash_1 = InfoHash::from([0; 20]);
        let infohash_2 = InfoHash::from([1; 20]);
        let s = format!(
            "magnet:?xt=urn:btih:{}&xt=urn:btih:{}",
            encode_hex(infohash_1),
            encode_hex(infohash_2),
        );
        let err = MagnetUri::parse(&s).unwrap_err();
        assert_eq!("Multiple infohashes found", err.to_string());
    }

    #[test]
    fn parse_multiple_identical_infohashes_present() {
        let infohash = InfoHash::from([0; 20]);
        let s = format!(
            "magnet:?xt=urn:btih:{}&xt=urn:btih:{}",
            encode_hex(infohash),
            encode_hex(infohash),
        );
        let magnet = MagnetUri::parse(&s).unwrap();
        assert_eq!(infohash, magnet.info_hash);
    }

    #[test]
    fn parse_invalid_peer_addr_no_err() {
        let infohash = InfoHash::from([0; 20]);
        let peer = "xxxyyyzzz";
        let s = format!("magnet:?xt=urn:btih:{}&x.pe={}", encode_hex(infohash), peer,);
        let magnet = MagnetUri::parse_lenient(&s).unwrap();
        assert_eq!(infohash, magnet.info_hash);
        assert!(magnet.peer_addrs.is_empty());
    }
}
