use std::net::SocketAddr;

use anyhow::{bail, ensure};
use ben::Parser;
use futures::{stream::FuturesUnordered, StreamExt};
use proto::{metainfo::MetaInfo, InfoHash, PeerId};
use sha1::Sha1;
use tokio::net::TcpStream;

use crate::Client;

pub async fn request_metadata(
    peers: impl Iterator<Item = &SocketAddr>,
    info_hash: &InfoHash,
    peer_id: &PeerId,
) -> anyhow::Result<MetaInfo> {
    let mut f = peers
        .map(|peer| request_metadata_from_peer(*peer, info_hash, peer_id))
        .collect::<FuturesUnordered<_>>();

    let parser = &mut Parser::new();
    while let Some(result) = f.next().await {
        match result {
            Ok(m) => {
                if let Ok(m) = MetaInfo::parse_with(&m, parser) {
                    return Ok(m);
                }
            }
            Err(e) => warn!("{}", e),
        }
    }

    bail!("Failed to retrieve metadata")
}

#[instrument(skip_all, fields(peer))]
async fn request_metadata_from_peer(
    peer: SocketAddr,
    info_hash: &InfoHash,
    peer_id: &PeerId,
) -> anyhow::Result<Vec<u8>> {
    let socket = TcpStream::connect(peer).await?;
    let mut client = Client::new(socket);
    client.send_handshake(info_hash, peer_id).await?;
    client.recv_handshake(info_hash).await?;
    client.send_unchoke();
    client.send_interested();

    let metadata = client.get_metadata().await?;
    let hash = Sha1::from(&metadata).digest().bytes();
    ensure!(hash == *info_hash, "Invalid metadata");
    Ok(metadata)
}
