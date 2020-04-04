use crate::announce::Tracker;
use crate::client::Client;
use crate::future::timeout;
use crate::metainfo::InfoHash;
use crate::msg::{Message, MetadataMsg};
use crate::peer::{Peer, PeerId};
use crate::torrent::Torrent;
use ben::{Encode, Node};
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use log::{debug, trace};
use std::collections::HashSet;
use std::net::SocketAddr;
use tokio::io::AsyncWriteExt;

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
    pub fn parse(s: &str) -> Result<Self, &'static str> {
        parser::MagnetUriParser::new().parse(s)
    }

    pub fn parse_lenient(s: &str) -> Result<Self, &'static str> {
        parser::MagnetUriParser::new_lenient().parse(s)
    }

    pub async fn request_metadata(&self, peer_id: Box<PeerId>) -> crate::Result<Torrent> {
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
                    if let Some(t) = self.read_info(&data) {
                        drop(futs);
                        trace!("Metadata requested successfully");
                        return Ok(Torrent {
                            peer_id,
                            info_hash: self.info_hash.clone(),
                            piece_len: t.piece_len,
                            length: t.length,
                            piece_hashes: t.piece_hashes,
                            name: t.name,
                            tracker_urls: self.tracker_urls.clone(),
                        });
                    }
                }
                Err(e) => debug!("Error : {}", e),
            }
        }

        Err("Metadata request failed".into())
    }

    fn read_info(&self, data: &[u8]) -> Option<TorrentInfo> {
        let node = Node::parse(&data).ok()?;
        let info_dict = node.as_dict()?;
        let length = info_dict.get_int(b"length")? as usize;
        let name = info_dict.get_str(b"name").unwrap_or_default().to_string();
        let piece_len = info_dict.get_int(b"piece length")? as usize;
        let piece_hashes = info_dict.get(b"pieces")?.data().to_vec();
        Some(TorrentInfo {
            piece_len,
            length,
            piece_hashes,
            name,
        })
    }

    async fn get_peers(&self, peer_id: &PeerId) -> Result<(Vec<Peer>, Vec<Peer>), &'static str> {
        debug!("Requesting peers");

        let mut futs: FuturesUnordered<_> = self
            .tracker_urls
            .iter()
            .map(|url| async move {
                let mut t = Tracker::new(url);
                t.announce(&self.info_hash, &peer_id).await
            })
            .collect();

        let mut peers = vec![];
        let mut peers6 = vec![];

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

        if peers.is_empty() && peers6.is_empty() {
            Err("No peers received from trackers")
        } else {
            Ok((peers, peers6))
        }
    }

    async fn try_get(&self, peer: &Peer, peer_id: &PeerId) -> crate::Result<Vec<u8>> {
        let mut client = Client::new_tcp(peer.addr).await?;
        client.handshake(&self.info_hash, peer_id).await?;
        client.conn.flush().await?;

        let mut ext_buf = vec![];
        let ext = loop {
            let msg = client.read_in_loop().await?;
            if let Message::Extended { .. } = msg {
                let ext = msg.read_ext(&mut client.conn, &mut ext_buf).await?;
                break ext;
            } else {
                msg.read_discard(&mut client.conn).await?;
            }
        };

        if !ext.is_handshake() {
            return Err("Expected Extended Handshake".into());
        }

        let metadata = ext
            .metadata()
            .ok_or("Peer doesn't support Metadata extension")?;

        debug!("{:?}", metadata);
        client.send_ext_handshake(metadata.id).await?;

        let mut remaining = metadata.len;
        let mut piece = 0;
        let mut buf = Vec::with_capacity(remaining);
        while remaining > 0 {
            let m = MetadataMsg::Request(piece);
            client.send_ext(metadata.id, m.encode_to_vec()).await?;
            client.conn.flush().await?;
            let msg = client.read_in_loop().await?;

            if let Message::Extended { .. } = msg {
                let ext = msg.read_ext(&mut client.conn, &mut ext_buf).await?;
                if ext.id != metadata.id {
                    return Err("Expected Metadata message".into());
                }

                let data = ext.data(piece)?;
                if data.len() > remaining {
                    return Err("Incorrect data length received".into());
                }

                buf.extend(data);
                remaining -= data.len();
                piece += 1;
            } else {
                msg.read_discard(&mut client.conn).await?;
            }
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
                    TRACKER_URL => {
                        magnet.tracker_urls.insert(value.to_string());
                    }
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

        let urls: HashSet<&str> = magnet.tracker_urls.iter().map(|s| &s[..]).collect();
        assert_eq!(hashset![tracker_url_1, tracker_url_2], urls);

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
