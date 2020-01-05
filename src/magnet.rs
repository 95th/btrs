use crate::metainfo::InfoHash;
use std::net::SocketAddr;

#[derive(Debug, Default)]
pub struct MagnetUri {
    infohash: InfoHash,
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
                            let infohash = build_infohash(&value[INFOHASH_PREFIX.len()..])?;

                            if has_ih && infohash != magnet.infohash {
                                return Err("Multiple infohashes found");
                            }

                            magnet.infohash = infohash;
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

    fn build_infohash(encoded: &str) -> Result<InfoHash, &'static str> {
        use data_encoding::{BASE32 as base32, HEXLOWER_PERMISSIVE as hex};

        let encoded = encoded.as_bytes();
        let mut id = InfoHash::default();

        match encoded.len() {
            40 => {
                hex.decode_mut(encoded, &mut id)
                    .map_err(|_| "Invalid hex string")?;
            }
            32 => {
                base32
                    .decode_mut(encoded, &mut id)
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
    use data_encoding::{BASE32 as base32, HEXLOWER_PERMISSIVE as hex};

    #[test]
    fn parse_hex_infohash() {
        let infohash = [12; 20];
        let s = format!("magnet:?xt=urn:btih:{}", hex.encode(&infohash));
        let magnet = MagnetUri::parse(&s).unwrap();
        assert_eq!(infohash, magnet.infohash);
    }

    #[test]
    fn parse_base32_infohash() {
        let infohash = [12; 20];
        let s = format!("magnet:?xt=urn:btih:{}", base32.encode(&infohash));
        let magnet = MagnetUri::parse(&s).unwrap();
        assert_eq!(infohash, magnet.infohash);
    }

    #[test]
    fn parse_all_params_present() {
        let infohash = [0; 20];
        let display_name = "xyz";
        let tracker_url_1 = "http://jupiter.gx/ann";
        let tracker_url_2 = "udp://jupiter.gx:1111";
        let peer_1 = "1.1.1.1:10000";
        let peer_2 = "2.2.2.2:10000";
        let s = format!(
            "magnet:?xt=urn:btih:{}&dn={}&tr={}&tr={}&x.pe={}&x.pe={}",
            hex.encode(&infohash),
            display_name,
            tracker_url_1,
            tracker_url_2,
            peer_1,
            peer_2
        );
        let magnet = MagnetUri::parse(&s).unwrap();
        assert_eq!(infohash, magnet.infohash);
        assert_eq!(display_name, magnet.display_name.unwrap());

        let urls: Vec<&str> = magnet.tracker_urls.iter().map(|s| &s[..]).collect();
        assert_eq!(&[tracker_url_1, tracker_url_2], &urls[..]);

        let peers: &[SocketAddr] = &[peer_1.parse().unwrap(), peer_2.parse().unwrap()];
        assert_eq!(peers, &magnet.peer_addrs[..]);
    }

    #[test]
    fn parse_only_infohash_present() {
        let infohash = [0; 20];
        let s = format!("magnet:?xt=urn:btih:{}", hex.encode(&infohash),);
        let magnet = MagnetUri::parse(&s).unwrap();
        assert_eq!(infohash, magnet.infohash);
    }

    #[test]
    fn parse_both_infohash_and_multihash_present() {
        let infohash = [0; 20];
        let multihash = [1; 20];
        let s = format!(
            "magnet:?xt=urn:btih:{}&xt=urn:btmh:{}",
            hex.encode(&infohash),
            hex.encode(&multihash),
        );
        let magnet = MagnetUri::parse(&s).unwrap();
        assert_eq!(infohash, magnet.infohash);
    }

    #[test]
    fn parse_multiple_infohashes_present() {
        let infohash_1 = [0; 20];
        let infohash_2 = [1; 20];
        let s = format!(
            "magnet:?xt=urn:btih:{}&xt=urn:btih:{}",
            hex.encode(&infohash_1),
            hex.encode(&infohash_2),
        );
        let err = MagnetUri::parse(&s).unwrap_err();
        assert_eq!("Multiple infohashes found", err);
    }

    #[test]
    fn parse_multiple_identical_infohashes_present() {
        let infohash = [0; 20];
        let s = format!(
            "magnet:?xt=urn:btih:{}&xt=urn:btih:{}",
            hex.encode(&infohash),
            hex.encode(&infohash),
        );
        let magnet = MagnetUri::parse(&s).unwrap();
        assert_eq!(infohash, magnet.infohash);
    }

    #[test]
    fn parse_invalid_peer_addr_no_err() {
        let infohash = [0; 20];
        let peer = "xxxyyyzzz";
        let s = format!(
            "magnet:?xt=urn:btih:{}&x.pe={}",
            hex.encode(&infohash),
            peer,
        );
        let magnet = MagnetUri::parse_lenient(&s).unwrap();
        assert_eq!(infohash, magnet.infohash);
        assert!(magnet.peer_addrs.is_empty());
    }
}
