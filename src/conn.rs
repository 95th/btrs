use crate::future::timeout;
use crate::metainfo::InfoHash;
use crate::peer::Peer;
use crate::torrent::TorrentFile;
use bencode::ValueRef;
use reqwest::Client;
use std::convert::TryInto;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Debug)]
pub struct AnnounceResponse {
    pub interval: usize,
    pub peers: Vec<Peer>,
    pub peers6: Vec<Peer>,
}

pub async fn announce(
    torrent: &TorrentFile,
    peer_id: &str,
    port: u16,
) -> crate::Result<AnnounceResponse> {
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
        .and_then(|v| v.as_int())
        .and_then(|n| n.try_into().ok())
        .unwrap_or(0);

    let peers = value.get("peers").and_then(|v| v.as_bytes()).unwrap_or(&[]);
    if peers.len() % 6 != 0 {
        Err("Invalid peer len")?;
    }

    let peers = peers.chunks_exact(6).map(Peer::v4).collect();

    let peers6 = value
        .get("peers6")
        .and_then(|v| v.as_bytes())
        .unwrap_or(&[]);
    if peers6.len() % 18 != 0 {
        return Err("Invalid peer len".into());
    }

    let peers6 = peers6.chunks_exact(18).map(Peer::v6).collect();

    Ok(AnnounceResponse {
        interval,
        peers,
        peers6,
    })
}

pub struct Handshake<'a> {
    pub extensions: [u8; 8],
    pub infohash: &'a InfoHash,
    pub peer_id: &'a str,
}

impl Handshake<'_> {
    pub fn as_bytes(&self) -> Vec<u8> {
        let mut v = vec![];
        v.push(0x13);
        v.extend(b"BitTorrent protocol");
        v.extend(&self.extensions);
        v.extend(self.infohash.as_ref());
        v.extend(self.peer_id.as_bytes());
        v
    }
}

pub async fn handshake(
    peer: &Peer,
    handshake: &Handshake<'_>,
    timeout_secs: u64,
) -> crate::Result<()> {
    let mut tcp = timeout(TcpStream::connect(peer.addr()), 3).await?;
    let msg = handshake.as_bytes();
    timeout(tcp.write_all(&msg), timeout_secs).await?;

    let mut v = vec![];
    timeout(tcp.read_to_end(&mut v), timeout_secs).await?;

    println!("{:?}, {:?}", msg, v);
    Ok(())
}
