use crate::torrent::TorrentFile;
use bencode::ValueRef;
use reqwest::Client;
use std::convert::TryInto;
use std::net::SocketAddr;

pub async fn connect(
    torrent: &TorrentFile,
    peer_id: &str,
    port: u16,
) -> crate::Result<AnnounceResponse> {
    println!("Announce: {}", torrent.announce);
    println!("InfoHash: {:x?}", torrent.info_hash);
    let url = format!(
        "{}?info_hash={}",
        torrent.announce,
        torrent.info_hash.encode_url()
    );
    let data = Client::new()
        .get(&url)
        .query(&[("peer_id", peer_id)])
        .query(&[("port", port)])
        .query(&[("uploaded", "0"), ("downloaded", "0"), ("compact", "1")])
        .send()
        .await?
        .bytes()
        .await?;
    let value = ValueRef::decode(&data).unwrap();
    let value = value.as_dict().ok_or("not a dict")?;
    let interval = value
        .get("interval")
        .ok_or("interval not found")?
        .as_int()
        .ok_or("interval not numeric")?
        .try_into()
        .unwrap_or(0);

    let peers = value.get("peers").and_then(|v| v.as_bytes()).unwrap_or(&[]);
    if peers.len() % 6 != 0 {
        Err("Invalid peer len")?;
    }

    let peers = peers
        .chunks_exact(6)
        .map(|b| {
            let ip: [u8; 4] = b[..4].try_into().unwrap();
            let port = u16::from_be_bytes(b[4..].try_into().unwrap());
            (ip, port).into()
        })
        .collect();

    let peers6 = value
        .get("peers6")
        .and_then(|v| v.as_bytes())
        .unwrap_or(&[]);
    if peers6.len() % 18 != 0 {
        return Err("Invalid peer len".into());
    }

    let peers6 = peers6
        .chunks_exact(18)
        .map(|b| {
            let ip: [u8; 16] = b[..16].try_into().unwrap();
            let port = u16::from_be_bytes(b[16..].try_into().unwrap());
            (ip, port).into()
        })
        .collect();

    Ok(AnnounceResponse {
        interval,
        peers,
        peers6,
    })
}

#[derive(Debug)]
pub struct AnnounceResponse {
    interval: usize,
    peers: Vec<SocketAddr>,
    peers6: Vec<SocketAddr>,
}
