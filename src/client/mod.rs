use crate::bitfield::BitField;
use crate::conn::Handshake;
use crate::future::timeout;
use crate::metainfo::InfoHash;
use crate::peer::{Peer, PeerId};
use tokio::net::TcpStream;

pub struct Client {
    conn: TcpStream,
    choked: bool,
    bitfield: BitField,
    peer: Peer,
    info_hash: InfoHash,
    peer_id: PeerId,
}

impl Client {
    pub async fn new(peer: Peer, info_hash: InfoHash, peer_id: PeerId) -> crate::Result<Self> {
        handshake(&peer, &info_hash, &peer_id).await?;
        todo!()
    }
}

async fn handshake(peer: &Peer, info_hash: &InfoHash, peer_id: &PeerId) -> crate::Result<()> {
    let mut conn = timeout(TcpStream::connect(peer.addr()), 3).await?;
    let handshake = Handshake::new(info_hash, peer_id);

    handshake.write(&mut conn).await?;
    let _ = handshake.read(&mut conn).await?;
    Ok(())
}
