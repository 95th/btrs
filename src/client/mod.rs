mod conn;
mod handshake;

use crate::bitfield::BitField;
use crate::client::handshake::Handshake;
use crate::metainfo::InfoHash;
use crate::msg::{Message, MetadataMsg};
use crate::peer::PeerId;
use ben::decode::Entry;
use ben::Encode;
pub use conn::{AsyncStream, Connection};
use std::io;
use std::net::SocketAddr;
use tokio::io::AsyncWriteExt;

pub struct Client<C = Connection> {
    pub conn: C,
    pub choked: bool,
    pub bitfield: BitField,
}

impl Client {
    pub async fn new_tcp(addr: SocketAddr) -> anyhow::Result<Self> {
        trace!("Create new TCP client to {:?}", addr);
        let conn = Connection::new_tcp(addr).await?;
        Ok(Client::new(conn))
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

    pub async fn handshake(
        &mut self,
        info_hash: &InfoHash,
        peer_id: &PeerId,
    ) -> anyhow::Result<()> {
        let mut handshake = Handshake::new(&mut self.conn, info_hash, peer_id);
        handshake.set_extended(true);
        handshake.write().await?;
        let result = handshake.read().await?;
        trace!("Handshake result: {:?}", result);
        Ok(())
    }

    pub async fn read(&mut self) -> anyhow::Result<Option<Message>> {
        trace!("Client::read");
        let msg = match Message::read(&mut self.conn).await? {
            Some(msg) => msg,
            None => return Ok(None), // Keep-alive
        };

        trace!("We got message: {:?}", msg);

        match msg {
            Message::Choke => {
                self.choked = true;
                Ok(None)
            }
            Message::Unchoke => {
                self.choked = false;
                Ok(None)
            }
            Message::Bitfield { len } => {
                let mut v = vec![0; len as usize];
                msg.read_bitfield(&mut self.conn, &mut v).await?;
                self.bitfield = v.into();
                Ok(None)
            }
            Message::Have { index } => {
                trace!("This guy has {} piece", index);
                self.bitfield.set(index as usize, true);
                Ok(None)
            }
            _ => Ok(Some(msg)),
        }
    }

    pub async fn read_in_loop(&mut self) -> anyhow::Result<Message> {
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

    pub async fn send_cancel(&mut self, index: u32, begin: u32, len: u32) -> io::Result<()> {
        let msg = Message::Cancel { index, begin, len };
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

    pub async fn send_bitfield(&mut self, buf: &[u8]) -> io::Result<()> {
        trace!("Send bitfield");
        let msg = Message::Bitfield {
            len: buf.len() as u32,
        };
        msg.write_buf(&mut self.conn, buf).await
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

    pub async fn send_ext_handshake(&mut self, id: u8) -> io::Result<()> {
        trace!("Send extended handshake");
        self.send_ext(0, MetadataMsg::Handshake(id).encode_to_vec())
            .await
    }

    pub async fn send_ext(&mut self, id: u8, data: Vec<u8>) -> io::Result<()> {
        let msg = Message::Extended {
            len: data.len() as u32,
        };

        trace!(
            "Send extended message : {:?} ; payload: {:?}",
            msg,
            ben::Parser::new().parse::<Entry>(&data).unwrap()
        );
        msg.write_ext(&mut self.conn, id, &data).await
    }

    pub async fn send_keep_alive(&mut self) -> anyhow::Result<()> {
        trace!("Send Keep-alive message");
        self.conn.write_u32(0).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use ben::ListEncoder;

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
    async fn interested() {
        let mut data = vec![];
        let mut tx = Client::new(Cursor::new(&mut data));
        tx.send_interested().await.unwrap();

        let mut rx = Client::new(Cursor::new(data));
        let msg = rx.read().await.unwrap().unwrap();
        assert_eq!(Message::Interested, msg);
    }

    #[tokio::test]
    async fn not_interested() {
        let mut data = vec![];
        let mut tx = Client::new(Cursor::new(&mut data));
        tx.send_not_interested().await.unwrap();

        let mut rx = Client::new(Cursor::new(data));
        let msg = rx.read().await.unwrap().unwrap();
        assert_eq!(Message::NotInterested, msg);
    }

    #[tokio::test]
    async fn have() {
        let mut data = vec![];
        let mut tx = Client::new(Cursor::new(&mut data));
        tx.send_have(1).await.unwrap();

        let mut rx = Client::new(Cursor::new(data));
        rx.bitfield = BitField::new(2);
        assert_eq!(Some(false), rx.bitfield.get(1));
        assert_eq!(None, rx.read().await.unwrap());
        assert_eq!(Some(true), rx.bitfield.get(1));
    }

    #[tokio::test]
    async fn bitfield() {
        let mut data = vec![];
        let mut tx = Client::new(Cursor::new(&mut data));
        let buf = b"1234";
        tx.send_bitfield(buf).await.unwrap();

        let mut rx = Client::new(Cursor::new(data));
        assert_eq!(None, rx.read().await.unwrap());
        assert_eq!(b"1234", &rx.bitfield.as_bytes());
    }

    #[tokio::test]
    async fn piece() {
        let mut data = vec![];
        let mut tx = Client::new(Cursor::new(&mut data));
        tx.send_piece(1, 0, b"1234").await.unwrap();

        let mut rx = Client::new(Cursor::new(data));
        let msg = rx.read().await.unwrap().unwrap();
        assert_eq!(
            Message::Piece {
                index: 1,
                begin: 0,
                len: 4,
            },
            msg
        );
        let mut buf = [0; 4];
        msg.read_piece(&mut rx.conn, &mut buf).await.unwrap();
        assert_eq!(b"1234", &buf);
    }

    #[tokio::test]
    async fn request() {
        let mut data = vec![];
        let mut tx = Client::new(Cursor::new(&mut data));
        tx.send_request(1, 0, 4).await.unwrap();

        let mut rx = Client::new(Cursor::new(data));
        let msg = rx.read().await.unwrap().unwrap();
        assert_eq!(
            Message::Request {
                index: 1,
                begin: 0,
                len: 4,
            },
            msg
        );
    }

    #[tokio::test]
    async fn cancel() {
        let mut data = vec![];
        let mut tx = Client::new(Cursor::new(&mut data));
        tx.send_cancel(1, 0, 4).await.unwrap();

        let mut rx = Client::new(Cursor::new(data));
        let msg = rx.read().await.unwrap().unwrap();
        assert_eq!(
            Message::Cancel {
                index: 1,
                begin: 0,
                len: 4,
            },
            msg
        );
    }

    #[tokio::test]
    async fn extended_handshake() {
        let mut data = vec![];
        let mut tx = Client::new(Cursor::new(&mut data));
        tx.send_ext_handshake(1).await.unwrap();

        let mut rx = Client::new(Cursor::new(data));
        let msg = rx.read().await.unwrap().unwrap();
        assert_eq!(Message::Extended { len: 45 }, msg);

        let mut buf = vec![];
        let mut parser = ben::Parser::new();
        let ext = msg
            .read_ext(&mut rx.conn, &mut buf, &mut parser)
            .await
            .unwrap();
        let expected = b"d1:md11:ut_metadatai1ee1:pi6881e4:reqqi500ee";
        assert_eq!(&expected[..], ext.body().as_raw_bytes());
    }

    #[tokio::test]
    async fn extended() {
        let mut data = vec![];
        let mut tx = Client::new(Cursor::new(&mut data));
        let mut payload = vec![];

        let mut list = ListEncoder::new(&mut payload);
        list.push(1);
        list.push(2);
        list.push(3);
        list.finish();

        tx.send_ext(1, payload).await.unwrap();

        println!("{:?}", data);

        let mut rx = Client::new(Cursor::new(data));
        let msg = rx.read().await.unwrap().unwrap();
        assert_eq!(Message::Extended { len: 12 }, msg);

        let mut buf = vec![];
        let mut parser = ben::Parser::new();
        let ext_msg = msg
            .read_ext(&mut rx.conn, &mut buf, &mut parser)
            .await
            .unwrap();
        assert_eq!(1, ext_msg.id);

        let list = ext_msg.body().as_list().unwrap();
        assert_eq!(
            vec![1, 2, 3],
            list.iter()
                .map(|n| n.as_int())
                .collect::<Option<Vec<_>>>()
                .unwrap()
        );
    }
}
