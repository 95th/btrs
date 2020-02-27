mod conn;
mod handshake;

use crate::bitfield::BitField;
use crate::client::handshake::Handshake;
use crate::metainfo::InfoHash;
use crate::msg::Message;
use crate::peer::PeerId;
use ben::Entry;
pub use conn::{AsyncStream, Connection};
use log::debug;
use log::trace;
use std::io;
use std::net::SocketAddr;
use tokio::io::{AsyncWriteExt, BufStream};
use tokio::net::TcpStream;

pub struct Client<C> {
    pub conn: C,
    pub choked: bool,
    pub bitfield: BitField,
}

impl Client<Connection> {
    pub async fn new_tcp(addr: SocketAddr) -> crate::Result<Self> {
        trace!("Create new TCP client to {:?}", addr);
        let conn = TcpStream::connect(addr).await?;
        Ok(Client::new(Connection::Tcp(BufStream::new(conn))))
    }
}

impl<C: AsyncStream> Client<C> {
    pub fn new(conn: C) -> Self {
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

    pub async fn send_request(&mut self, index: u32, begin: u32, len: u32) -> io::Result<()> {
        let msg = Message::Request { index, begin, len };
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

    pub async fn send_have(&mut self, index: u32) -> io::Result<()> {
        trace!("Send have for piece: {}", index);
        let msg = Message::Have { index };
        msg.write(&mut self.conn).await
    }

    pub async fn send_piece(&mut self, index: u32, begin: u32, buf: &[u8]) -> io::Result<()> {
        trace!("Send have for piece: {}", index);
        let msg = Message::Piece {
            index,
            begin,
            len: buf.len() as u32,
        };
        msg.write_buf(&mut self.conn, buf).await
    }

    pub async fn send_ext_handshake(&mut self) -> io::Result<()> {
        trace!("Send extended handshake");
        Message::Extended { len: 0 }.write(&mut self.conn).await
    }

    pub async fn send_ext(&mut self, id: u8, value: Entry) -> io::Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[tokio::test]
    async fn keep_alive() {
        let mut data = vec![];
        let mut tx = Client::new(Cursor::new(&mut data));
        tx.send_keep_alive().await.unwrap();

        let mut rx = Client::new(Cursor::new(&mut data));
        assert_eq!(None, rx.read().await.unwrap());
    }

    #[tokio::test]
    async fn choke() {
        let mut data = vec![];
        let mut tx = Client::new(Cursor::new(&mut data));
        tx.send_choke().await.unwrap();

        let mut rx = Client::new(Cursor::new(data));
        rx.choked = false;
        assert_eq!(None, rx.read().await.unwrap());
        assert!(rx.choked);
    }

    #[tokio::test]
    async fn unchoke() {
        let mut data = vec![];
        let mut tx = Client::new(Cursor::new(&mut data));
        tx.send_unchoke().await.unwrap();

        let mut rx = Client::new(Cursor::new(data));
        assert!(rx.choked);
        assert_eq!(None, rx.read().await.unwrap());
        assert!(!rx.choked);
    }

    #[tokio::test]
    async fn have() {
        let mut data = vec![];
        let mut tx = Client::new(Cursor::new(&mut data));
        tx.send_have(1).await.unwrap();

        let mut rx = Client::new(Cursor::new(data));
        rx.bitfield = BitField::new(2);
        assert!(!rx.bitfield.get(1));
        assert_eq!(None, rx.read().await.unwrap());
        assert!(rx.bitfield.get(1));
    }

    #[tokio::test]
    async fn piece() {
        let mut data = vec![];
        let mut tx = Client::new(Cursor::new(&mut data));
        tx.send_piece(1, 0, b"1234").await.unwrap();

        let mut rx = Client::new(Cursor::new(data));
        let piece = rx.read().await.unwrap().unwrap();
        assert_eq!(
            Message::Piece {
                index: 1,
                begin: 0,
                len: 4,
            },
            piece
        );
        let mut buf = [0; 4];
        piece.read_piece(&mut rx.conn, &mut buf).await.unwrap();
        assert_eq!(b"1234", &buf);
    }

    #[tokio::test]
    async fn request() {
        let mut data = vec![];
        let mut tx = Client::new(Cursor::new(&mut data));
        tx.send_request(1, 0, 4).await.unwrap();

        let mut rx = Client::new(Cursor::new(data));
        let piece = rx.read().await.unwrap().unwrap();
        assert_eq!(
            Message::Request {
                index: 1,
                begin: 0,
                len: 4,
            },
            piece
        );
    }
}
