mod handshake;

use crate::bitfield::BitField;
use crate::client::handshake::Handshake;
use crate::metainfo::InfoHash;
use crate::msg::{self, Message, MessageKind};
use crate::peer::{Peer, PeerId};
use bencode::Value;
use log::trace;
use std::fmt::Debug;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

pub trait Connection: AsyncRead + AsyncWrite + Send + Debug + Unpin {}

impl<T> Connection for T where T: AsyncRead + AsyncWrite + Send + Debug + Unpin {}

#[derive(Debug)]
pub struct Client {
    conn: Box<dyn Connection>,
    pub choked: bool,
    pub bitfield: BitField,
}

impl Client {
    pub async fn new_tcp(peer: &Peer) -> crate::Result<Self> {
        trace!("Create new TCP client to {:?}", peer);
        let conn = TcpStream::connect(peer.addr()).await?;
        Ok(Client::new(Box::new(conn)))
    }

    pub fn new(conn: Box<dyn Connection>) -> Self {
        Self {
            conn,
            choked: true,
            bitfield: BitField::default(),
        }
    }

    pub async fn handshake(&mut self, info_hash: &InfoHash, peer_id: &PeerId) -> crate::Result<()> {
        let mut handshake = Handshake::new(&mut self.conn, info_hash, peer_id);
        handshake.set_extended(true);
        handshake.write().await?;
        let result = handshake.read().await?;
        trace!("Handshake result: {:?}", result);
        Ok(())
    }

    pub async fn read(&mut self) -> crate::Result<Option<Message>> {
        trace!("Read message");
        msg::read(&mut self.conn).await
    }

    pub async fn recv_bitfield(&mut self) -> crate::Result<()> {
        trace!("Receive Bitfield");
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
        trace!(
            "Send Piece request: index: {}, begin: {}, length: {}",
            index,
            begin,
            length
        );
        msg::request(index as u32, begin as u32, length as u32)
            .write(&mut self.conn)
            .await
    }

    pub async fn send_interested(&mut self) -> crate::Result<()> {
        trace!("Send interested");
        msg::interested().write(&mut self.conn).await
    }

    pub async fn send_not_interested(&mut self) -> crate::Result<()> {
        trace!("Send not interested");
        msg::not_interested().write(&mut self.conn).await
    }

    pub async fn send_choke(&mut self) -> crate::Result<()> {
        trace!("Send choke");
        msg::choke().write(&mut self.conn).await
    }

    pub async fn send_unchoke(&mut self) -> crate::Result<()> {
        trace!("Send unchoke");
        msg::unchoke().write(&mut self.conn).await
    }

    pub async fn send_have(&mut self, index: usize) -> crate::Result<()> {
        trace!("Send have for piece: {}", index);
        msg::have(index as u32).write(&mut self.conn).await
    }

    pub async fn send_ext_handshake(&mut self) -> crate::Result<()> {
        trace!("Send extended handshake");
        msg::ext_handshake().write(&mut self.conn).await
    }

    pub async fn send_ext(&mut self, id: u8, value: &Value) -> crate::Result<()> {
        trace!("Send extended message");
        msg::ext(id, value).write(&mut self.conn).await
    }
}
