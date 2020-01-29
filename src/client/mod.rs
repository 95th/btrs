mod handshake;

use crate::bitfield::BitField;
use crate::client::handshake::Handshake;
use crate::metainfo::InfoHash;
use crate::msg::{self, Message, MessageKind};
use crate::peer::{Peer, PeerId};
use bencode::Value;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

#[derive(Debug)]
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
        let conn = TcpStream::connect(peer.addr()).await?;
        Client::new(conn, peer, info_hash, peer_id).await
    }
}

impl<C> Client<C>
where
    C: AsyncRead + AsyncWrite + Unpin,
{
    pub async fn new(
        mut conn: C,
        peer: Peer,
        info_hash: InfoHash,
        peer_id: PeerId,
    ) -> crate::Result<Self> {
        handshake(&mut conn, &info_hash, &peer_id).await?;
        Ok(Self {
            conn,
            choked: true,
            bitfield: BitField::default(),
            peer,
            info_hash,
            peer_id,
        })
    }

    pub async fn read(&mut self) -> crate::Result<Option<Message>> {
        msg::read(&mut self.conn).await
    }

    pub async fn recv_bitfield(&mut self) -> crate::Result<()> {
        match self.read().await? {
            Some(Message {
                kind: MessageKind::Bitfield,
                payload,
            }) => {
                self.bitfield = payload.into();
                Ok(())
            }
            _ => Err("Invalid message: Expected Bitfield".into()),
        }
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

    pub async fn send_extended_handshake(&mut self, value: &Value) -> crate::Result<()> {
        msg::extended_handshake(value).write(&mut self.conn).await
    }

    pub async fn send_extended(&mut self, id: u8, value: &Value) -> crate::Result<()> {
        msg::extended(id, value).write(&mut self.conn).await
    }
}

async fn handshake<C>(conn: &mut C, info_hash: &InfoHash, peer_id: &PeerId) -> crate::Result<()>
where
    C: AsyncRead + AsyncWrite + Unpin,
{
    let mut handshake = Handshake::new(info_hash, peer_id);
    handshake.set_extensions(true);
    handshake.write(conn).await?;
    let _ = handshake.read(conn).await?;
    Ok(())
}
