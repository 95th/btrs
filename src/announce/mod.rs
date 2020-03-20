use crate::future::timeout;
use crate::metainfo::InfoHash;
use crate::peer::{Peer, PeerId};
use log::{trace, warn};
use std::collections::HashSet;
use std::time::{Duration, Instant};

mod http;
mod udp;

const MIN_TRACKER_INTERVAL: u64 = 10;

#[derive(Debug)]
pub enum Event {
    None,
    Completed,
    Started,
    Stopped,
}

pub struct Tracker {
    pub url: String,
    next_announce: Instant,
    interval: u64,
}

impl Tracker {
    pub fn new(url: String) -> Self {
        Self {
            url,
            next_announce: Instant::now(),
            interval: MIN_TRACKER_INTERVAL,
        }
    }

    pub async fn announce(
        mut self,
        info_hash: &InfoHash,
        peer_id: &PeerId,
    ) -> (Option<AnnounceResponse>, Self) {
        tokio::time::delay_until(self.next_announce.into()).await;

        trace!("Announce to {}", self.url);
        let req = AnnounceRequest::new(&self.url, info_hash, peer_id, 6881);
        let resp = match timeout(req.send(), 3).await {
            Ok(r) => {
                self.interval = MIN_TRACKER_INTERVAL.max(r.interval);
                Some(r)
            }
            Err(e) => {
                warn!("Announce failed: {}", e);
                None
            }
        };
        self.next_announce = Instant::now() + Duration::from_secs(self.interval);
        (resp, self)
    }
}

#[derive(Debug)]
pub struct AnnounceResponse {
    pub interval: u64,
    pub peers: HashSet<Peer>,
    pub peers6: HashSet<Peer>,
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
