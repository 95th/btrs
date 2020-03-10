use crate::metainfo::InfoHash;
use crate::peer::{Peer, PeerId};

mod http;
mod udp;

#[derive(Debug)]
pub enum Event {
    None,
    Completed,
    Started,
    Stopped,
}

#[derive(Debug)]
pub struct AnnounceResponse {
    pub interval: usize,
    pub peers: Vec<Peer>,
    pub peers6: Vec<Peer>,
}

pub struct AnnounceRequest<'a> {
    pub url: &'a str,
    pub info_hash: &'a InfoHash,
    pub peer_id: &'a PeerId,
    pub port: u16,
    pub downloaded: u64,
    pub left: u64,
    pub uploaded: u64,
    pub event: Event,
}

impl AnnounceRequest<'_> {
    pub fn new<'a>(
        url: &'a str,
        info_hash: &'a InfoHash,
        peer_id: &'a PeerId,
        port: u16,
    ) -> AnnounceRequest<'a> {
        AnnounceRequest {
            url,
            info_hash,
            peer_id,
            port,
            downloaded: 0,
            left: 0,
            uploaded: 0,
            event: Event::None,
        }
    }
}

impl AnnounceRequest<'_> {
    pub async fn send(self) -> crate::Result<AnnounceResponse> {
        if self.url.starts_with("http") {
            http::announce(self).await
        } else if self.url.starts_with("udp") {
            udp::announce(self).await
        } else {
            Err("Unsupported tracker URL".into())
        }
    }
}
