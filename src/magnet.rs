use crate::announce::{announce, AnnounceResponse};
use crate::client::Client;
use crate::future::timeout;
use crate::metainfo::InfoHash;
use crate::peer::{Peer, PeerId};
use crate::torrent::Torrent;
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use log::debug;
use std::net::SocketAddr;

#[derive(Debug, Default)]
pub struct MagnetUri {
    info_hash: InfoHash,
    display_name: Option<String>,
    tracker_urls: Vec<String>,
    peer_addrs: Vec<SocketAddr>,
}

impl MagnetUri {
    pub fn parse(s: &str) -> Result<Self, &'static str> {
        parser::MagnetUriParser::new().parse(s)
    }

    pub fn parse_lenient(s: &str) -> Result<Self, &'static str> {
        parser::MagnetUriParser::new_lenient().parse(s)
    }

    pub async fn request_metadata(&self, peer_id: PeerId) -> Result<Torrent, &'static str> {
        debug!("Requesting peers");
        let mut peer_futures: FuturesUnordered<_> = self
            .tracker_urls
            .iter()
            .map(|url| timeout(self.get_peers(url, &peer_id), 10))
            .collect();

        let mut peers = vec![];
        let mut peers6 = vec![];

        loop {
            match peer_futures.next().await {
                Some(Ok((p, p6))) => {
                    peers.extend(p);
                    peers6.extend(p6);
                }
                Some(Err(e)) => debug!("Error: {}", e),
                None => break,
            }
        }

        debug!("Got {} v4 peers and {} v6 peers", peers.len(), peers6.len());

        if peers.is_empty() && peers6.is_empty() {
            return Err("No peers received from trackers");
        }

        let mut client_futures: FuturesUnordered<_> = peers
            .iter()
            .chain(peers6.iter())
            .map(|p| {
                let p = p.clone();
                async {
                    timeout(self.try_get(p.clone(), peer_id), 10)
                        .await
                        .map_err(|e| (p, e))
                }
            })
            .collect();

        loop {
            match client_futures.next().await {
                Some(Ok(_)) => {
                    return Ok(Torrent {
                        peers,
                        peers6,
                        info_hash: self.info_hash.clone(),
                        peer_id,
                        piece_len: 0,
                        piece_hashes: vec![],
                        length: 0,
                        name: "".to_string(),
                    })
                }
                Some(Err((p, e))) => debug!("Error for {:?} : {}", p, e),
                None => break,
            }
        }

        Err("Metadata request failed")
    }

    async fn get_peers(
        &self,
        url: &str,
        peer_id: &PeerId,
    ) -> crate::Result<(Vec<Peer>, Vec<Peer>)> {
        match announce(url, &self.info_hash, &peer_id, 6881).await? {
            AnnounceResponse { peers, peers6, .. } => Ok((peers, peers6)),
        }
    }

    async fn try_get(&self, peer: Peer, peer_id: PeerId) -> crate::Result<()> {
        let ih = self.info_hash.clone();

        debug!("Create client to {:?}", peer);
        let mut client = Client::new_tcp(peer, ih, peer_id).await?;

        debug!("Send extension handshake");
        client.send_extended_handshake().await?;

        debug!("Recv extended message");
        loop {
            let msg = match client.read().await? {
                Some(m) => m,
                // Keep-alive
                None => continue,
            };
            let ext = msg.parse_extended()?;

            println!("Received: {:#?}", ext.value);
            break;
        }
        Ok(())
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

        pub fn parse(&self, uri: &str) -> Result<MagnetUri, &'static str> {
            let url = Url::parse(uri).unwrap();
            if url.scheme() != SCHEME {
                return Err("Incorrect scheme");
            }

            let mut magnet = MagnetUri::default();
            let mut has_ih = false;
            for (key, value) in url.query_pairs() {
                match &key[..] {
                    TORRENT_ID => {
                        if value.starts_with(INFOHASH_PREFIX) {
                            let info_hash = build_info_hash(&value[INFOHASH_PREFIX.len()..])?;

                            if has_ih && info_hash != magnet.info_hash {
                                return Err("Multiple infohashes found");
                            }

                            magnet.info_hash = info_hash;
                            has_ih = true;
                        }
                    }
                    DISPLAY_NAME => magnet.display_name = Some(value.to_string()),
                    TRACKER_URL => magnet.tracker_urls.push(value.to_string()),
                    PEER => match value.parse() {
                        Ok(addr) => magnet.peer_addrs.push(addr),
                        Err(_) => {
                            if self.strict {
                                return Err("Invalid peer addr");
                            }
                        }
                    },
                    _ => {}
                }
            }
            if has_ih {
                Ok(magnet)
            } else {
                Err("No infohash found")
            }
        }
    }

    fn build_info_hash(encoded: &str) -> Result<InfoHash, &'static str> {
        use data_encoding::{BASE32 as base32, HEXLOWER_PERMISSIVE as hex};

        let encoded = encoded.as_bytes();
        let mut id = InfoHash::default();

        match encoded.len() {
            40 => {
                hex.decode_mut(encoded, id.as_mut())
                    .map_err(|_| "Invalid hex string")?;
            }
            32 => {
                base32
                    .decode_mut(encoded, id.as_mut())
                    .map_err(|_| "Invalid base 32 string")?;
            }
            _ => return Err("Invalid infohash length"),
        }

        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_infohash() {
        let infohash = InfoHash::from([12; 20]);
        let s = format!("magnet:?xt=urn:btih:{}", infohash.encode_hex());
        let magnet = MagnetUri::parse(&s).unwrap();
        assert_eq!(infohash, magnet.info_hash);
    }

    #[test]
    fn parse_base32_infohash() {
        let infohash = InfoHash::from([12; 20]);
        let s = format!("magnet:?xt=urn:btih:{}", infohash.encode_base32());
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
            infohash.encode_hex(),
            display_name,
            tracker_url_1,
            tracker_url_2,
            peer_1,
            peer_2
        );
        let magnet = MagnetUri::parse(&s).unwrap();
        assert_eq!(infohash, magnet.info_hash);
        assert_eq!(display_name, magnet.display_name.unwrap());

        let urls: Vec<&str> = magnet.tracker_urls.iter().map(|s| &s[..]).collect();
        assert_eq!(&[tracker_url_1, tracker_url_2], &urls[..]);

        let peers: &[SocketAddr] = &[peer_1.parse().unwrap(), peer_2.parse().unwrap()];
        assert_eq!(peers, &magnet.peer_addrs[..]);
    }

    #[test]
    fn parse_only_infohash_present() {
        let infohash = InfoHash::from([0; 20]);
        let s = format!("magnet:?xt=urn:btih:{}", infohash.encode_hex());
        let magnet = MagnetUri::parse(&s).unwrap();
        assert_eq!(infohash, magnet.info_hash);
    }

    #[test]
    fn parse_both_infohash_and_multihash_present() {
        let infohash = InfoHash::from([0; 20]);
        let multihash = InfoHash::from([1; 20]);
        let s = format!(
            "magnet:?xt=urn:btih:{}&xt=urn:btmh:{}",
            infohash.encode_hex(),
            multihash.encode_hex(),
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
            infohash_1.encode_hex(),
            infohash_2.encode_hex(),
        );
        let err = MagnetUri::parse(&s).unwrap_err();
        assert_eq!("Multiple infohashes found", err);
    }

    #[test]
    fn parse_multiple_identical_infohashes_present() {
        let infohash = InfoHash::from([0; 20]);
        let s = format!(
            "magnet:?xt=urn:btih:{}&xt=urn:btih:{}",
            infohash.encode_hex(),
            infohash.encode_hex(),
        );
        let magnet = MagnetUri::parse(&s).unwrap();
        assert_eq!(infohash, magnet.info_hash);
    }

    #[test]
    fn parse_invalid_peer_addr_no_err() {
        let infohash = InfoHash::from([0; 20]);
        let peer = "xxxyyyzzz";
        let s = format!(
            "magnet:?xt=urn:btih:{}&x.pe={}",
            infohash.encode_hex(),
            peer,
        );
        let magnet = MagnetUri::parse_lenient(&s).unwrap();
        assert_eq!(infohash, magnet.info_hash);
        assert!(magnet.peer_addrs.is_empty());
    }
}
