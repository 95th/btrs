#[macro_use]
extern crate tracing;

use std::io;

use anyhow::{bail, ensure};
use proto::{buf::RecvBuffer, conn::Connection, event::Event, msg::Packet};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub use proto::*;

pub trait AsyncStream: AsyncRead + AsyncWrite + Unpin {}

impl<T: AsyncRead + AsyncWrite + Unpin> AsyncStream for T {}

pub struct Client<Stream> {
    stream: Stream,
    conn: Connection,
    recv_buf: RecvBuffer,
}

impl<Stream> Client<Stream>
where
    Stream: AsyncStream,
{
    pub fn new(stream: Stream) -> Self {
        Self {
            stream,
            conn: Connection::new(),
            recv_buf: RecvBuffer::with_capacity(12),
        }
    }

    pub async fn send_handshake(
        &mut self,
        info_hash: &InfoHash,
        peer_id: &PeerId,
    ) -> anyhow::Result<()> {
        debug!("Send handshake");
        self.conn.send_handshake(info_hash, peer_id);
        self.flush().await
    }

    pub async fn recv_handshake(&mut self, info_hash: &InfoHash) -> anyhow::Result<PeerId> {
        debug!("Recv handshake");

        let mut buf = [0; 68];
        self.stream.read_exact(&mut buf).await?;
        self.conn.recv_handshake(info_hash, buf)
    }

    pub async fn read_packet(&mut self) -> anyhow::Result<Option<Packet<'_>>> {
        let len = self.read_packet_bytes().await?;
        if len == 0 {
            // Keep-alive
            return Ok(None);
        }

        let header_len = Packet::header_len(self.recv_buf.peek());
        ensure!(len >= header_len + 1, "Invalid packet length");

        let buf = self.recv_buf.read(len);
        let packet = self.conn.recv_packet(buf);
        flush(&mut self.stream, &mut self.conn).await?;
        Ok(packet)
    }

    pub async fn wait_for_unchoke(&mut self) -> anyhow::Result<()> {
        while self.conn.is_choked() {
            self.read_packet().await?;
        }
        Ok(())
    }

    pub async fn get_metadata(&mut self) -> anyhow::Result<Vec<u8>> {
        debug!("Request metadata");

        while !self.conn.ext_handshaked() {
            self.read_packet().await?;
        }

        if !self.conn.request_metadata() {
            bail!("Metadata request not supported");
        }

        loop {
            self.read_packet().await?;

            while let Some(event) = self.conn.poll_event() {
                match event {
                    Event::Metadata(metadata) => return Ok(metadata),
                }
            }
        }
    }

    /// Receive one packet from the peer with length header removed.
    /// Hence returns an empty buffer if it is a keep-alive message.
    async fn read_packet_bytes(&mut self) -> anyhow::Result<usize> {
        self.read_bytes(4).await?;
        let len = self.recv_buf.read_array();
        let len = u32::from_be_bytes(*len) as usize;
        if len == 0 {
            return Ok(0);
        }

        ensure!(len <= 1024 * 1024, "Packet too large: {}", len);
        self.read_bytes(len).await?;
        Ok(len)
    }

    pub fn send_request(&mut self, index: u32, begin: u32, len: u32) {
        self.conn.send_request(index, begin, len);
    }

    pub fn send_have(&mut self, index: u32) {
        self.conn.send_have(index);
    }

    pub fn send_unchoke(&mut self) {
        self.conn.send_unchoke();
    }

    pub fn send_interested(&mut self) {
        self.conn.send_interested();
    }

    pub fn send_not_interested(&mut self) {
        self.conn.send_not_interested();
    }

    pub fn send_piece(&mut self, index: u32, begin: u32, data: &[u8]) {
        self.conn.send_piece(index, begin, data);
    }

    pub async fn flush(&mut self) -> anyhow::Result<()> {
        flush(&mut self.stream, &mut self.conn).await
    }

    pub fn is_choked(&self) -> bool {
        self.conn.is_choked()
    }

    async fn read_bytes(&mut self, len: usize) -> io::Result<()> {
        loop {
            let b = self.recv_buf.write_reserve(len);

            // No further read required
            if b.is_empty() {
                return Ok(());
            }

            let n = self.stream.read(b).await?;
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "early EOF"));
            }

            self.recv_buf.advance_write(n);
        }
    }
}

async fn flush(stream: &mut impl AsyncStream, conn: &mut Connection) -> anyhow::Result<()> {
    stream.write_all(&conn.get_send_buf()).await?;
    stream.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        pin::Pin,
        task::{Context, Poll},
    };

    use futures::{
        channel::mpsc::{self, Receiver, Sender},
        join, ready, SinkExt, StreamExt,
    };
    use proto::msg::{Packet, PieceBlock};
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

    use crate::Client;

    struct Peer {
        tx: Sender<Vec<u8>>,
        rx: Receiver<Vec<u8>>,
        remaining: Vec<u8>,
    }

    impl Peer {
        pub fn create_pair() -> (Peer, Peer) {
            let (t1, r1) = mpsc::channel(200);
            let (t2, r2) = mpsc::channel(200);
            let p1 = Peer {
                tx: t1,
                rx: r2,
                remaining: vec![],
            };
            let p2 = Peer {
                tx: t2,
                rx: r1,
                remaining: vec![],
            };
            (p1, p2)
        }
    }

    impl AsyncRead for Peer {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            let data = if self.remaining.is_empty() {
                match ready!(self.rx.poll_next_unpin(cx)) {
                    Some(data) => data,
                    None => return Poll::Ready(Ok(())),
                }
            } else {
                std::mem::take(&mut self.remaining)
            };

            if data.len() <= buf.capacity() {
                buf.put_slice(&data);
            } else {
                buf.put_slice(&data[..buf.capacity()]);
                self.remaining = data[buf.capacity()..].to_vec();
            }

            Poll::Ready(Ok(()))
        }
    }

    fn err() -> io::Error {
        io::Error::from(io::ErrorKind::BrokenPipe)
    }

    impl AsyncWrite for Peer {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            ready!(self.tx.poll_ready(cx)).map_err(|_| err())?;
            self.tx.start_send_unpin(buf.to_vec()).map_err(|_| err())?;
            Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            self.tx.poll_flush_unpin(cx).map_err(|_| err())
        }

        fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            self.tx.poll_close_unpin(cx).map_err(|_| err())
        }
    }

    #[tokio::test]
    async fn handshake() {
        let (a, b) = Peer::create_pair();
        let f1 = async move {
            let mut c = Client::new(a);
            c.send_handshake(&[0; 20], &[1; 20]).await.unwrap();
            let p = c.recv_handshake(&[0; 20]).await.unwrap();
            assert_eq!(p, [2; 20]);
        };

        let f2 = async move {
            let mut c = Client::new(b);
            c.send_handshake(&[0; 20], &[2; 20]).await.unwrap();
            let p = c.recv_handshake(&[0; 20]).await.unwrap();
            assert_eq!(p, [1; 20]);
        };

        join!(f1, f2);
    }

    #[tokio::test]
    async fn send_piece() {
        let (a, b) = Peer::create_pair();
        let f1 = async move {
            let mut c = Client::new(a);
            c.send_piece(1, 2, b"hello");
            c.flush().await.unwrap();
        };

        let f2 = async move {
            let mut c = Client::new(b);
            let p = c.read_packet().await.unwrap().unwrap();
            assert_eq!(
                p,
                Packet::Piece(PieceBlock {
                    index: 1,
                    begin: 2,
                    data: b"hello"
                })
            )
        };

        join!(f1, f2);
    }

    #[tokio::test]
    async fn send_interested_and_receive_unchoke() {
        let (a, b) = Peer::create_pair();
        let f1 = async move {
            let mut c = Client::new(a);
            assert!(c.conn.is_choked());
            c.send_interested();
            c.flush().await.unwrap();
            c.read_packet().await.unwrap();
            assert!(!c.conn.is_choked());
        };

        let f2 = async move {
            let mut c = Client::new(b);
            c.read_packet().await.unwrap();
        };

        join!(f1, f2);
    }

    #[tokio::test]
    async fn send_not_interested_and_receive_choke() {
        let (a, b) = Peer::create_pair();
        let f1 = async move {
            let mut c = Client::new(a);
            assert!(c.conn.is_choked());
            c.send_interested();
            c.flush().await.unwrap();
            c.read_packet().await.unwrap();
            assert!(!c.conn.is_choked());
            c.send_not_interested();
            c.flush().await.unwrap();
            c.read_packet().await.unwrap();
            assert!(c.conn.is_choked());
        };

        let f2 = async move {
            let mut c = Client::new(b);
            c.read_packet().await.unwrap();
            c.read_packet().await.unwrap();
        };

        join!(f1, f2);
    }
}
