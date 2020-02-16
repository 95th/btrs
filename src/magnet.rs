use crate::announce::announce;
use crate::client::Client;
use crate::future::timeout;
use crate::metainfo::InfoHash;
use crate::msg::MetadataMsg;
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

    pub async fn request_metadata(&self, peer_id: Box<PeerId>) -> Result<Torrent, &'static str> {
        let (peers, peers6) = self.get_peers(&peer_id).await?;

        let mut futs: FuturesUnordered<_> = peers
            .iter()
            .chain(peers6.iter())
            .map(|peer| self.try_get(peer, &peer_id))
            .map(|fut| timeout(fut, 10))
            .collect();

        while let Some(result) = futs.next().await {
            match result {
                Ok(data) => {
                    drop(futs);
                    debug!("Got metadata: {:?}", data);
                    return Ok(Torrent {
                        peers,
                        peers6,
                        info_hash: self.info_hash.clone(),
                        peer_id,
                        piece_len: 0,
                        piece_hashes: vec![],
                        length: 0,
                        name: "".to_string(),
                    });
                }
                Err(e) => debug!("Error : {}", e),
            }
        }

        Err("Metadata request failed")
    }

    async fn get_peers(&self, peer_id: &PeerId) -> Result<(Vec<Peer>, Vec<Peer>), &'static str> {
        debug!("Requesting peers");

        let mut peers = vec![];
        let mut peers6 = vec![];

        let mut futs: FuturesUnordered<_> = self
            .tracker_urls
            .iter()
            .map(|url| announce(url, &self.info_hash, &peer_id, 6881))
            .map(|fut| timeout(fut, 10))
            .collect();

        while let Some(result) = futs.next().await {
            match result {
                Ok(r) => {
                    peers.extend(r.peers);
                    peers6.extend(r.peers6);
                }
                Err(e) => debug!("Error: {}", e),
            }
        }

        debug!("Got {} v4 peers and {} v6 peers", peers.len(), peers6.len());

        if peers.is_empty() && peers6.is_empty() {
            Err("No peers received from trackers")
        } else {
            Ok((peers, peers6))
        }
    }

    async fn try_get(&self, peer: &Peer, peer_id: &PeerId) -> crate::Result<Vec<u8>> {
        let mut client = Client::new_tcp(peer.addr).await?;
        client.handshake(&self.info_hash, peer_id).await?;
        client.send_ext_handshake().await?;

        let msg = client.read_in_loop().await?;
        let ext = msg.parse_ext()?;

        if !ext.is_handshake() {
            return Err("Expected Extended Handshake".into());
        }

        let metadata = ext
            .metadata()
            .ok_or("Peer doesn't support Metadata extension")?;

        debug!("{:?}", metadata);

        let mut remaining = metadata.len;
        let mut piece = 0;
        let mut buf = vec![0; remaining];
        while remaining > 0 {
            let m = MetadataMsg::Request(piece);
            client.send_ext(metadata.id, m.into()).await?;
            let msg = client.read_in_loop().await?;

            let ext = msg.parse_ext()?;
            if ext.id != metadata.id {
                return Err("Expected Metadata message".into());
            }

            let data = ext.data(piece)?;
            buf.extend(data);
            remaining -= data.len();
            piece += 1;
        }

        Ok(buf)
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
