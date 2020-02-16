mod handshake;

use crate::bitfield::BitField;
use crate::client::handshake::Handshake;
use crate::metainfo::InfoHash;
use crate::msg::{self, Message, MessageKind};
use crate::peer::PeerId;
use ben::Encoder;
use log::debug;
use log::trace;
use std::net::SocketAddr;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

pub trait Connection: AsyncRead + AsyncWrite + Unpin {}

impl<T> Connection for T where T: AsyncRead + AsyncWrite + Unpin {}

pub struct Client {
    conn: Box<dyn Connection>,
    pub choked: bool,
    pub bitfield: BitField,
}

impl Client {
    pub async fn new_tcp(addr: SocketAddr) -> crate::Result<Self> {
        trace!("Create new TCP client to {:?}", addr);
        let conn = TcpStream::connect(addr).await?;
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
        let msg = match msg::read(&mut self.conn).await? {
            Some(msg) => msg,
            None => return Ok(None), // Keep-alive
        };

        debug!("We got message: {:?}", msg.kind);

        match msg.kind {
            MessageKind::Choke => {
                self.choked = true;
                return Ok(None);
            }
            MessageKind::Unchoke => {
                self.choked = false;
                return Ok(None);
            }
            MessageKind::Bitfield => {
                debug!("Received bitfield: {:?}", msg.payload);
                self.bitfield = msg.payload.into();
                return Ok(None);
            }
            MessageKind::Have => {
                let index = msg.parse_have()?;
                debug!("This guy has {} piece", index);
                self.bitfield.set(index, true);
                return Ok(None);
            }
            _ => return Ok(Some(msg)),
        }
    }

    pub async fn read_in_loop(&mut self) -> crate::Result<Message> {
        loop {
            if let Some(msg) = self.read().await? {
                return Ok(msg);
            }
        }
    }

    pub async fn recv_bitfield(&mut self) -> crate::Result<()> {
        trace!("Receive Bitfield");
        match msg::read(&mut self.conn).await? {
            Some(Message {
                kind: MessageKind::Bitfield,
                payload,
            }) => {
                self.bitfield = payload.into();
                Ok(())
            }
            msg => Err(format!("Invalid message: Expected Bitfield, got: {:?}", msg).into()),
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

    pub async fn send_ext(&mut self, id: u8, value: Encoder) -> crate::Result<()> {
        trace!("Send extended message");
        msg::ext(id, value).write(&mut self.conn).await
    }

    pub async fn send_keep_alive(&mut self) -> crate::Result<()> {
        trace!("Send Keep-alive message");
        self.conn.write_u32(0).await?;
        Ok(())
    }
}
