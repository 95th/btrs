use crate::future::timeout;
use crate::metainfo::InfoHash;
use crate::peer::{Peer, PeerId};
use log::{trace, warn};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

mod http;
mod udp;

use http::HttpTrackerMgr;
use udp::UdpTrackerMgr;
use udp::UdpTrackerMgrHandle;

const MIN_TRACKER_INTERVAL: u64 = 10;

#[derive(Debug, Clone, Copy)]
pub enum Event {
    None,
    Completed,
    Started,
    Stopped,
}

pub struct Tracker {
    mgr: TrackerMgr,
    pub url: String,
    resolved_addr: Option<SocketAddr>,
    next_announce: Instant,
    interval: u64,
}

impl Tracker {
    pub fn new(url: String, mgr: TrackerMgr) -> Self {
        Self {
            mgr,
            url,
            resolved_addr: None,
            next_announce: Instant::now(),
            interval: MIN_TRACKER_INTERVAL,
        }
    }

    pub async fn announce(
        mut self,
        info_hash: &InfoHash,
        peer_id: &PeerId,
    ) -> (Option<AnnounceResponse>, Tracker) {
        tokio::time::delay_until(self.next_announce.into()).await;

        trace!("Announce to {}", self.url);
        let req = AnnounceRequest::new(&self.url, self.resolved_addr, info_hash, peer_id, 6881);
        let resp = match timeout(self.mgr.announce(req), 3).await {
            Ok(r) => {
                self.interval = MIN_TRACKER_INTERVAL.max(r.interval);
                self.resolved_addr = r.resolved_addr;
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
    pub resolved_addr: Option<SocketAddr>,
    pub interval: u64,
    pub peers: HashSet<Peer>,
    pub peers6: HashSet<Peer>,
}

#[derive(Debug)]
pub struct AnnounceRequest {
    pub url: String,

    /// Used by UDP tracker announcement to save expensive DNS queries
    pub resolved_addr: Option<SocketAddr>,

    pub info_hash: InfoHash,
    pub peer_id: PeerId,
    pub port: u16,
    pub downloaded: u64,
    pub left: u64,
    pub uploaded: u64,
    pub event: Event,
}

impl AnnounceRequest {
    pub fn new(
        url: &str,
        resolved_addr: Option<SocketAddr>,
        info_hash: &InfoHash,
        peer_id: &PeerId,
        port: u16,
    ) -> AnnounceRequest {
        AnnounceRequest {
            url: url.to_owned(),
            resolved_addr,
            info_hash: info_hash.clone(),
            peer_id: peer_id.clone(),
            port,
            downloaded: 0,
            left: 0,
            uploaded: 0,
            event: Event::None,
        }
    }
}

#[derive(Clone)]
pub struct TrackerMgr {
    udp: UdpTrackerMgrHandle,
    http: HttpTrackerMgr,
}

impl TrackerMgr {
    pub async fn new() -> crate::Result<TrackerMgr> {
        let mut mgr = UdpTrackerMgr::new().await?;
        let udp = mgr.handle();
        tokio::spawn(async move { mgr.listen().await });

        Ok(TrackerMgr {
            udp,
            http: HttpTrackerMgr,
        })
    }

    pub async fn announce(&mut self, req: AnnounceRequest) -> crate::Result<AnnounceResponse> {
        if req.url.starts_with("http") {
            self.http.announce(req).await
        } else if req.url.starts_with("udp") {
            self.udp.announce(req).await
        } else {
            Err("Unsupported tracker URL".into())
        }
    }
}
