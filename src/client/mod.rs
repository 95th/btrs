use crate::bitfield::BitField;
use crate::conn::Handshake;
use crate::future::timeout;
use crate::metainfo::InfoHash;
use crate::msg::{self, Message, MessageKind};
use crate::peer::{Peer, PeerId};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

pub struct Client<C>
where
    C: AsyncRead + AsyncWrite + Unpin,
{
    conn: C,
    choked: bool,
    bitfield: BitField,
    peer: Peer,
    info_hash: InfoHash,
    peer_id: PeerId,
}

impl<C> Client<C>
where
    C: AsyncRead + AsyncWrite + Unpin,
{
    pub async fn new_tcp(
        peer: Peer,
        info_hash: InfoHash,
        peer_id: PeerId,
    ) -> crate::Result<Client<TcpStream>> {
        let mut conn = TcpStream::connect(peer.addr()).await?;
        timeout(handshake(&mut conn, &info_hash, &peer_id), 3).await?;
        let bitfield = timeout(recv_bitfield(&mut conn), 5).await?;
        Ok(Client {
            conn,
            choked: true,
            bitfield,
            peer,
            info_hash,
            peer_id,
        })
    }

    pub async fn read_msg(&mut self) -> crate::Result<Option<Message>> {
        Ok(msg::read(&mut self.conn).await?)
    }
}

async fn handshake<C>(conn: &mut C, info_hash: &InfoHash, peer_id: &PeerId) -> crate::Result<()>
where
    C: AsyncRead + AsyncWrite + Unpin,
{
    let handshake = Handshake::new(info_hash, peer_id);

    handshake.write(conn).await?;
    let _ = handshake.read(conn).await?;
    Ok(())
}

async fn recv_bitfield<R>(rdr: &mut R) -> crate::Result<BitField>
where
    R: AsyncRead + Unpin,
{
    if let Some(Message {
        kind: MessageKind::Bitfield,
        payload,
    }) = msg::read(rdr).await?
    {
        Ok(payload.into())
    } else {
        Err("Invalid message")?
    }
}
