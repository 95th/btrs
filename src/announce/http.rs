use crate::announce::{AnnounceRequest, AnnounceResponse};
use crate::peer;
use anyhow::Context;
use ben::decode::Dict;
use ben::Parser;
use client::InfoHash;
use percent_encoding::{percent_encode, PercentEncode, NON_ALPHANUMERIC};
use reqwest::Client;
use std::collections::HashSet;
use std::net::SocketAddr;

fn encode_url(infohash: &InfoHash) -> PercentEncode {
    percent_encode(infohash, NON_ALPHANUMERIC)
}

pub async fn announce(req: AnnounceRequest<'_>) -> anyhow::Result<AnnounceResponse> {
    let peer_id = std::str::from_utf8(&req.peer_id[..]).unwrap();
    let info_hash_encoded = encode_url(&req.info_hash);
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
    let mut parser = Parser::new();
    let value = parser.parse::<Dict>(&data)?;
    let interval = value.get_int("interval").unwrap_or(0);

    let peers = match value.get("peers") {
        Some(peers) if peers.is_list() => {
            let mut v = hashset![];
            for peer in peers.as_list().unwrap().iter() {
                let peer = peer.as_dict().context("Peer not a dict")?;
                let ip = peer
                    .get_str("ip")
                    .context("IP not present")
                    .and_then(|v| v.parse().context("Invalid IP/DNS name"))?;
                let port = peer.get_int("port").context("Port not present")?;
                v.insert(SocketAddr::new(ip, port));
            }
            v
        }
        Some(peers) => {
            let peers = peers.as_bytes().unwrap_or_default();
            anyhow::ensure!(peers.len() % 6 == 0, "Invalid peer len");
            peers.chunks_exact(6).map(peer::v4).collect()
        }
        None => hashset![],
    };

    debug!("Found {} peers (v4): {:?}", peers.len(), peers);

    let peers6 = value.get_bytes("peers6").unwrap_or_default();
    anyhow::ensure!(peers6.len() % 18 == 0, "Invalid peer len");

    let peers6: HashSet<_> = peers6.chunks_exact(18).map(peer::v6).collect();
    debug!("Found {} peers (v6): {:?}", peers6.len(), peers6);

    Ok(AnnounceResponse {
        interval,
        peers,
        peers6,
        resolved_addr: None,
    })
}
