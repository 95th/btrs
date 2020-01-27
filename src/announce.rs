use crate::peer::{Peer, PeerId};
use crate::torrent::TorrentFile;
use bencode::ValueRef;
use log::debug;
use reqwest::Client;
use std::convert::TryInto;

#[derive(Debug)]
pub struct AnnounceResponse {
    pub interval: usize,
    pub peers: Vec<Peer>,
    pub peers6: Vec<Peer>,
}

pub async fn announce(
    torrent: &TorrentFile,
    peer_id: &PeerId,
    port: u16,
) -> crate::Result<AnnounceResponse> {
    let peer_id = std::str::from_utf8(peer_id).unwrap();
    let url = format!(
        "{}?info_hash={}",
        torrent.announce,
        torrent.info_hash.encode_url()
    );
    let data = Client::new()
        .get(&url)
        .query(&[("peer_id", peer_id)])
        .query(&[("port", port)])
        .query(&[("uploaded", "0"), ("downloaded", "0"), ("compact", "1")]) // prefer compact peer list
        .send()
        .await?
        .bytes()
        .await?;
    let value = ValueRef::decode(&data).unwrap();
    let value = value.as_dict().ok_or("not a dict")?;
    let interval = value
        .get("interval")
        .and_then(|v| v.as_int())
        .and_then(|n| n.try_into().ok())
        .unwrap_or(0);

    let peers = match value.get("peers") {
        Some(peers) if peers.is_list() => {
            let mut v = vec![];
            for peer in peers.as_list().unwrap() {
                let peer = peer.as_dict().ok_or("Peer not a dict")?;
                let ip = peer
                    .get("ip")
                    .and_then(|ip| ip.as_str())
                    .ok_or("IP not present")
                    .and_then(|v| v.parse().map_err(|_| "Invalid IP/DNS name"))?;
                let port = peer
                    .get("port")
                    .ok_or("Port not present")
                    .and_then(|port| port.as_int().ok_or("Invalid port number"))?;
                v.push(Peer::new(ip, port as u16));
            }
            v
        }
        Some(peers) => {
            let peers = peers.as_bytes().unwrap_or(&[]);
            if peers.len() % 6 != 0 {
                return Err("Invalid peer len".into());
            }

            peers.chunks_exact(6).map(Peer::v4).collect()
        }
        None => vec![],
    };

    debug!("Found {} peers (v4)", peers.len());

    let peers6 = value
        .get("peers6")
        .and_then(|v| v.as_bytes())
        .unwrap_or(&[]);
    if peers6.len() % 18 != 0 {
        return Err("Invalid peer len".into());
    }

    let peers6: Vec<_> = peers6.chunks_exact(18).map(Peer::v6).collect();
    debug!("Found {} peers (v6)", peers6.len());

    Ok(AnnounceResponse {
        interval,
        peers,
        peers6,
    })
}
