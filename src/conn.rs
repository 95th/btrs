use crate::metainfo::InfoHash;
use crate::peer::{Peer, PeerId};
use crate::torrent::TorrentFile;
use bencode::ValueRef;
use log::debug;
use reqwest::Client;
use std::convert::{TryFrom, TryInto};
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const PROTOCOL: &[u8] = b"BitTorrent protocol";

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

pub struct Handshake<'a> {
    pub extensions: [u8; 8],
    pub info_hash: &'a InfoHash,
    pub peer_id: &'a PeerId,
}

impl<'a> Handshake<'a> {
    const LEN: usize = 68;

    pub fn new(info_hash: &'a InfoHash, peer_id: &'a PeerId) -> Self {
        Self {
            peer_id,
            info_hash,
            extensions: Default::default(),
        }
    }

    pub async fn write<W>(&self, writer: &mut W) -> io::Result<()>
    where
        W: AsyncWrite + Unpin,
    {
        writer.write_all(&[19]).await?;
        writer.write_all(PROTOCOL).await?;
        writer.write_all(&self.extensions).await?;
        writer.write_all(self.info_hash.as_ref()).await?;
        writer.write_all(self.peer_id).await?;
        Ok(())
    }

    pub async fn read<R>(&self, reader: &mut R) -> crate::Result<PeerId>
    where
        R: AsyncRead + Unpin,
    {
        let mut buf = [0; Handshake::LEN];
        reader.read_exact(&mut buf).await?;

        if buf[0] as usize != PROTOCOL.len() {
            Err("Invalid length")?;
        }

        if &buf[1..20] != PROTOCOL {
            Err("Invalid Protocol")?;
        }

        let info_hash = InfoHash::try_from(&buf[28..48])?;
        if self.info_hash != &info_hash {
            Err("InfoHash mismatch")?;
        }

        let peer_id = buf[48..68].try_into().unwrap();
        Ok(peer_id)
    }
}
