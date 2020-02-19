mod handshake;

use crate::bitfield::BitField;
use crate::client::handshake::Handshake;
use crate::metainfo::InfoHash;
use crate::msg::Message;
use crate::peer::PeerId;
use ben::Encoder;
use log::debug;
use log::trace;
use std::io;
use std::net::SocketAddr;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

pub trait Connection: AsyncRead + AsyncWrite + Unpin {}

impl<T> Connection for T where T: AsyncRead + AsyncWrite + Unpin {}

pub struct Client {
    pub conn: Box<dyn Connection>,
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
        trace!("Client::read");
        let msg = match Message::read(&mut self.conn).await? {
            Some(msg) => msg,
            None => return Ok(None), // Keep-alive
        };

        debug!("We got message: {:?}", msg);

        match msg {
            Message::Choke => {
                self.choked = true;
                return Ok(None);
            }
            Message::Unchoke => {
                self.choked = false;
                return Ok(None);
            }
            Message::Bitfield { len } => {
                let mut v = vec![0; len as usize];
                msg.read_bitfield(&mut self.conn, &mut v).await?;
                self.bitfield = v.into();
                return Ok(None);
            }
            Message::Have { index } => {
                debug!("This guy has {} piece", index);
                self.bitfield.set(index as usize, true);
                return Ok(None);
            }
            _ => return Ok(Some(msg)),
        }
    }

    pub async fn read_in_loop(&mut self) -> crate::Result<Message> {
        trace!("Client::read_in_loop");
        loop {
            if let Some(msg) = self.read().await? {
                return Ok(msg);
            }
        }
    }

    pub async fn recv_bitfield(&mut self) -> crate::Result<()> {
        trace!("Receive Bitfield");
        let msg = self.read().await?;
        match msg {
            Some(msg) => {
                msg.read_bitfield(&mut self.conn, self.bitfield.as_bytes_mut())
                    .await?;
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
    ) -> io::Result<()> {
        let msg = Message::Request {
            index: index as u32,
            begin: begin as u32,
            len: length as u32,
        };
        trace!("Send {:?}", msg);
        msg.write(&mut self.conn).await
    }

    pub async fn send_interested(&mut self) -> io::Result<()> {
        trace!("Send interested");
        Message::Interested.write(&mut self.conn).await
    }

    pub async fn send_not_interested(&mut self) -> io::Result<()> {
        trace!("Send not interested");
        Message::NotInterested.write(&mut self.conn).await
    }

    pub async fn send_choke(&mut self) -> io::Result<()> {
        trace!("Send choke");
        Message::Choke.write(&mut self.conn).await
    }

    pub async fn send_unchoke(&mut self) -> io::Result<()> {
        trace!("Send unchoke");
        Message::Unchoke.write(&mut self.conn).await
    }

    pub async fn send_have(&mut self, index: usize) -> io::Result<()> {
        trace!("Send have for piece: {}", index);
        let msg = Message::Have {
            index: index as u32,
        };
        msg.write(&mut self.conn).await
    }

    pub async fn send_ext_handshake(&mut self) -> io::Result<()> {
        trace!("Send extended handshake");
        Message::Extended { len: 0 }.write(&mut self.conn).await
    }

    pub async fn send_ext(&mut self, id: u8, value: Encoder) -> io::Result<()> {
        trace!("Send extended message");
        let data = value.to_vec();
        let msg = Message::Extended {
            len: data.len() as u32,
        };
        msg.write_ext(&mut self.conn, id, &data).await
    }

    pub async fn send_keep_alive(&mut self) -> crate::Result<()> {
        trace!("Send Keep-alive message");
        self.conn.write_u32(0).await?;
        Ok(())
    }
}
