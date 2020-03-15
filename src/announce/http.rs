use crate::announce::{AnnounceRequest, AnnounceResponse};
use crate::peer::Peer;
use ben::Node;
use log::debug;
use reqwest::Client;
use std::collections::HashSet;
use std::convert::TryInto;

pub async fn announce(req: AnnounceRequest<'_>) -> crate::Result<AnnounceResponse> {
    let peer_id = std::str::from_utf8(&req.peer_id[..]).unwrap();
    let info_hash_encoded = req.info_hash.encode_url();
    debug!("Infohash Encoded: {}", info_hash_encoded);
    let url = format!("{}?info_hash={}", req.url, info_hash_encoded);
    let data = Client::new()
        .get(&url)
        .query(&[("peer_id", peer_id)])
        .query(&[("port", req.port)])
        .query(&[("uploaded", "0"), ("downloaded", "0"), ("compact", "1")]) // prefer compact peer list
        .send()
        .await?
        .bytes()
        .await?;

    debug!("Announce response: {:?}", data);
    let value = Node::parse(&data)?;
    let value = value.as_dict().ok_or("not a dict")?;
    let interval = value
        .get(b"interval")
        .and_then(|v| v.as_int())
        .and_then(|n| n.try_into().ok())
        .unwrap_or(0);

    let peers = match value.get(b"peers") {
        Some(peers) if peers.is_list() => {
            let mut v = HashSet::new();
            for peer in peers.as_list().unwrap().iter() {
                let peer = peer.as_dict().ok_or("Peer not a dict")?;
                let ip = peer
                    .get_str(b"ip")
                    .ok_or("IP not present")
                    .and_then(|v| v.parse().map_err(|_| "Invalid IP/DNS name"))?;
                let port = peer.get_int(b"port").ok_or("Port not present")?;
                v.insert(Peer::new(ip, port as u16));
            }
            v
        }
        Some(peers) => {
            let peers = peers.data();
            if peers.len() % 6 != 0 {
                return Err("Invalid peer len".into());
            }

            peers.chunks_exact(6).map(Peer::v4).collect()
        }
        None => HashSet::new(),
    };

    debug!("Found {} peers (v4): {:?}", peers.len(), peers);

    let peers6 = value.get(b"peers6").map(|v| v.data()).unwrap_or_default();
    if peers6.len() % 18 != 0 {
        return Err("Invalid peer len".into());
    }

    let peers6: HashSet<_> = peers6.chunks_exact(18).map(Peer::v6).collect();
    debug!("Found {} peers (v6): {:?}", peers6.len(), peers6);

    Ok(AnnounceResponse {
        interval,
        peers,
        peers6,
    })
}
