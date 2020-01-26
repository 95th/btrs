use crate::bitfield::BitField;
use crate::conn::Handshake;
use crate::future::timeout;
use crate::metainfo::InfoHash;
use crate::msg::{self, Message, MessageKind};
use crate::peer::{Peer, PeerId};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

pub struct Client<C> {
    conn: C,
    pub choked: bool,
    pub bitfield: BitField,
    peer: Peer,
    info_hash: InfoHash,
    peer_id: PeerId,
}

impl Client<TcpStream> {
    pub async fn new_tcp(peer: Peer, info_hash: InfoHash, peer_id: PeerId) -> crate::Result<Self> {
        let mut conn = TcpStream::connect(peer.addr()).await?;
        timeout(handshake(&mut conn, &info_hash, &peer_id), 3).await?;
        let bitfield = timeout(recv_bitfield(&mut conn), 5).await?;
        Ok(Self {
            conn,
            choked: true,
            bitfield,
            peer,
            info_hash,
            peer_id,
        })
    }
}

impl<C> Client<C>
where
    C: AsyncRead + AsyncWrite + Unpin,
{
    pub async fn read(&mut self) -> crate::Result<Option<Message>> {
        msg::read(&mut self.conn).await
    }

    pub async fn send_request(
        &mut self,
        index: usize,
        begin: usize,
        length: usize,
    ) -> crate::Result<()> {
        msg::request(index as u32, begin as u32, length as u32)
            .write(&mut self.conn)
            .await
    }

    pub async fn send_interested(&mut self) -> crate::Result<()> {
        msg::interested().write(&mut self.conn).await
    }

    pub async fn send_not_interested(&mut self) -> crate::Result<()> {
        msg::not_interested().write(&mut self.conn).await
    }

    pub async fn send_choke(&mut self) -> crate::Result<()> {
        msg::choke().write(&mut self.conn).await
    }

    pub async fn send_unchoke(&mut self) -> crate::Result<()> {
        msg::unchoke().write(&mut self.conn).await
    }

    pub async fn send_have(&mut self, index: usize) -> crate::Result<()> {
        msg::have(index as u32).write(&mut self.conn).await
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
