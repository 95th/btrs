mod handshake;

use crate::bitfield::BitField;
use crate::client::handshake::Handshake;
use crate::metainfo::InfoHash;
use crate::msg::Message;
use crate::peer::PeerId;
use ben::Entry;
use bytes::{Buf, BufMut};
use log::debug;
use log::trace;
use std::io;
use std::mem::MaybeUninit;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufStream};
use tokio::net::TcpStream;

pub trait AsyncStream: AsyncRead + AsyncWrite + Unpin {}

impl<T: AsyncRead + AsyncWrite + Unpin> AsyncStream for T {}

pub enum Connection {
    Tcp(BufStream<TcpStream>),
    Other(Box<dyn AsyncStream>),
}

pub struct Client {
    pub conn: Connection,
    pub choked: bool,
    pub bitfield: BitField,
}

impl AsyncRead for Client {
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [MaybeUninit<u8>]) -> bool {
        match &self.conn {
            Connection::Tcp(c) => c.prepare_uninitialized_buffer(buf),
            Connection::Other(c) => c.prepare_uninitialized_buffer(buf),
        }
    }

    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        match &mut self.conn {
            Connection::Tcp(c) => Pin::new(c).poll_read(cx, buf),
            Connection::Other(c) => Pin::new(c).poll_read(cx, buf),
        }
    }

    fn poll_read_buf<B: BufMut>(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut B,
    ) -> Poll<io::Result<usize>>
    where
        Self: Sized,
    {
        match &mut self.conn {
            Connection::Tcp(c) => Pin::new(c).poll_read_buf(cx, buf),
            Connection::Other(c) => Pin::new(c).poll_read_buf(cx, buf),
        }
    }
}

impl AsyncWrite for Client {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        match &mut self.conn {
            Connection::Tcp(c) => Pin::new(c).poll_write(cx, buf),
            Connection::Other(c) => Pin::new(c).poll_write(cx, buf),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        match &mut self.conn {
            Connection::Tcp(c) => Pin::new(c).poll_flush(cx),
            Connection::Other(c) => Pin::new(c).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        match &mut self.conn {
            Connection::Tcp(c) => Pin::new(c).poll_shutdown(cx),
            Connection::Other(c) => Pin::new(c).poll_shutdown(cx),
        }
    }

    fn poll_write_buf<B: Buf>(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut B,
    ) -> Poll<Result<usize, io::Error>>
    where
        Self: Sized,
    {
        match &mut self.conn {
            Connection::Tcp(c) => Pin::new(c).poll_write_buf(cx, buf),
            Connection::Other(c) => Pin::new(c).poll_write_buf(cx, buf),
        }
    }
}

impl Client {
    pub async fn new_tcp(addr: SocketAddr) -> crate::Result<Self> {
        trace!("Create new TCP client to {:?}", addr);
        let conn = TcpStream::connect(addr).await?;
        Ok(Client::new(Connection::Tcp(BufStream::new(conn))))
    }

    pub fn new(conn: Connection) -> Self {
        Self {
            conn,
            choked: true,
            bitfield: BitField::default(),
        }
    }

    pub async fn handshake(&mut self, info_hash: &InfoHash, peer_id: &PeerId) -> crate::Result<()> {
        let mut handshake = Handshake::new(self, info_hash, peer_id);
        handshake.set_extended(true);
        handshake.write().await?;
        let result = handshake.read().await?;
        trace!("Handshake result: {:?}", result);
        Ok(())
    }

    pub async fn read(&mut self) -> crate::Result<Option<Message>> {
        trace!("Client::read");
        let msg = match Message::read(self).await? {
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
                msg.read_bitfield(self, &mut v).await?;
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
        msg.write(self).await
    }

    pub async fn send_interested(&mut self) -> io::Result<()> {
        trace!("Send interested");
        Message::Interested.write(self).await
    }

    pub async fn send_not_interested(&mut self) -> io::Result<()> {
        trace!("Send not interested");
        Message::NotInterested.write(self).await
    }

    pub async fn send_choke(&mut self) -> io::Result<()> {
        trace!("Send choke");
        Message::Choke.write(self).await
    }

    pub async fn send_unchoke(&mut self) -> io::Result<()> {
        trace!("Send unchoke");
        Message::Unchoke.write(self).await
    }

    pub async fn send_have(&mut self, index: u32) -> io::Result<()> {
        trace!("Send have for piece: {}", index);
        let msg = Message::Have { index };
        msg.write(self).await
    }

    pub async fn send_ext_handshake(&mut self) -> io::Result<()> {
        trace!("Send extended handshake");
        Message::Extended { len: 0 }.write(self).await
    }

    pub async fn send_ext(&mut self, id: u8, value: Entry) -> io::Result<()> {
        trace!("Send extended message");
        let data = value.to_vec();
        let msg = Message::Extended {
            len: data.len() as u32,
        };
        msg.write_ext(self, id, &data).await
    }

    pub async fn send_keep_alive(&mut self) -> crate::Result<()> {
        trace!("Send Keep-alive message");
        self.write_u32(0).await?;
        Ok(())
    }
}
