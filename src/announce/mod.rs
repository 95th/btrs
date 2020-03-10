use crate::metainfo::InfoHash;
use crate::peer::{Peer, PeerId};

#[derive(Debug)]
pub struct AnnounceResponse {
    pub interval: usize,
    pub peers: Vec<Peer>,
    pub peers6: Vec<Peer>,
}

mod http;
mod udp;

pub async fn announce(
    url: &str,
    info_hash: &InfoHash,
    peer_id: &PeerId,
    port: u16,
) -> crate::Result<AnnounceResponse> {
    if url.starts_with("http") {
        http::announce(url, info_hash, peer_id, port).await
    } else if url.starts_with("udp") {
        udp::announce(url, info_hash, peer_id, port).await
    } else {
        Err("Unsupported tracker URL".into())
    }
}
