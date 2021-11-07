#[macro_use]
extern crate tracing;

use anyhow::{ensure, Context};
use ben::Parser;
use futures::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use proto::{
    conn::Connection,
    ext::{ExtendedMessage, Metadata, MetadataMsg},
    handshake::Handshake,
    msg::Packet,
    InfoHash, PeerId,
};

pub use proto;

pub struct Client<Stream> {
    stream: Stream,
    conn: Connection,
    parser: Parser,
}

impl<Stream> Client<Stream>
where
    Stream: AsyncRead + AsyncWrite + Unpin,
{
    pub fn new(stream: Stream) -> Self {
        Self {
            stream,
            conn: Connection::new(),
            parser: Parser::new(),
        }
    }

    pub async fn handshake(
        &mut self,
        info_hash: InfoHash,
        peer_id: PeerId,
    ) -> anyhow::Result<PeerId> {
        debug!("Begin handshake");
        let mut handshake = Handshake::new(info_hash, peer_id);
        handshake.set_extended(true);

        self.stream.write_all(handshake.as_bytes()).await?;
        self.stream.flush().await?;

        let mut response = Handshake::default();

        debug!("Wait for handshake response");
        self.stream.read_exact(response.as_bytes_mut()).await?;
        handshake.verify(&response)?;

        debug!("Handshake succeeded");
        Ok(response.peer_id)
    }

    pub async fn read_packet<'a>(
        &mut self,
        buf: &'a mut Vec<u8>,
    ) -> anyhow::Result<Option<Packet<'a>>> {
        let mut b = [0; 4];

        trace!("Read packet length");
        self.stream.read_exact(&mut b).await?;

        let len = u32::from_be_bytes(b) as usize;
        trace!("Packet length: {}", len);

        if len == 0 {
            // Keep-alive
            return Ok(None);
        }

        ensure!(len <= 1024 * 1024, "Packet too large");

        buf.resize(len, 0);
        self.stream.read_exact(buf).await?;

        let header_len = Packet::header_len(buf[0]);
        ensure!(len >= header_len + 1, "Invalid packet length");

        let packet = self.conn.read_packet(buf);
        trace!("Read packet: {:?}", packet);

        self.flush().await?;
        Ok(packet)
    }

    pub async fn get_metadata(&mut self) -> anyhow::Result<Vec<u8>> {
        debug!("Request metadata");
        let buf = &mut Vec::new();
        let data = loop {
            trace!("Try to read an extended handshake");
            if let Some(Packet::Extended(data)) = self.read_packet(buf).await? {
                break data;
            }
        };

        let ext = ExtendedMessage::parse(data, &mut self.parser)?;
        trace!("Extended handshake message: {:?}", ext);

        ensure!(ext.is_handshake(), "Expected extended handshake");

        let metadata = ext.metadata().context("Metadata extension not supported")?;

        self.conn
            .send_extended(metadata.id, MetadataMsg::Handshake(metadata.id));
        self.flush().await?;

        self.read_metadata(metadata, buf).await
    }

    async fn read_metadata(
        &mut self,
        metadata: Metadata,
        buf: &mut Vec<u8>,
    ) -> anyhow::Result<Vec<u8>> {
        debug!("Read metadata");
        let mut remaining = metadata.len;
        let mut piece = 0;
        let mut out_buf = Vec::new();

        while remaining > 0 {
            trace!("Send metadata piece request: {}", piece);

            self.conn
                .send_extended(metadata.id, MetadataMsg::Request(piece));
            self.flush().await?;

            let data = loop {
                if let Some(Packet::Extended(data)) = self.read_packet(buf).await? {
                    break data;
                }
            };

            let ext = ExtendedMessage::parse(data, &mut self.parser)?;
            trace!("Got piece response: {:?}", ext);
            anyhow::ensure!(ext.id == metadata.id, "Expected Metadata message");

            let data = ext.data(piece)?;
            anyhow::ensure!(data.len() <= remaining, "Incorrect data length received");

            out_buf.extend_from_slice(data);
            remaining -= data.len();
            piece += 1;
        }

        Ok(out_buf)
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
        let send_buf = self.conn.get_send_buf();
        if !send_buf.is_empty() {
            self.stream.write_all(&send_buf).await?;
        }
        Ok(())
    }
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
        executor::block_on,
        join, ready, AsyncRead, AsyncWrite, SinkExt, StreamExt,
    };
    use proto::msg::Packet;

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
            buf: &mut [u8],
        ) -> Poll<io::Result<usize>> {
            let data = if self.remaining.is_empty() {
                match ready!(self.rx.poll_next_unpin(cx)) {
                    Some(data) => data,
                    None => return Poll::Ready(Ok(0)),
                }
            } else {
                std::mem::take(&mut self.remaining)
            };

            if data.len() <= buf.len() {
                buf[..data.len()].copy_from_slice(&data);
                Poll::Ready(Ok(data.len()))
            } else {
                buf.copy_from_slice(&data[..buf.len()]);
                self.remaining = data[buf.len()..].to_vec();
                cx.waker().wake_by_ref();
                Poll::Ready(Ok(buf.len()))
            }
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

        fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            self.tx.poll_close_unpin(cx).map_err(|_| err())
        }
    }

    #[test]
    fn handshake() {
        block_on(async {
            let (a, b) = Peer::create_pair();
            let f1 = async move {
                let mut c = Client::new(a);
                c.handshake([0; 20], [1; 20]).await.unwrap();
            };

            let f2 = async move {
                let mut c = Client::new(b);
                c.handshake([0; 20], [2; 20]).await.unwrap();
            };

            join!(f1, f2);
        })
    }

    #[test]
    fn send_piece() {
        block_on(async {
            let (a, b) = Peer::create_pair();
            let f1 = async move {
                let mut c = Client::new(a);
                c.send_piece(1, 2, b"hello");
                c.flush().await.unwrap();
            };

            let f2 = async move {
                let mut c = Client::new(b);
                let b = &mut vec![];
                let p = c.read_packet(b).await.unwrap().unwrap();
                assert_eq!(
                    p,
                    Packet::Piece {
                        index: 1,
                        begin: 2,
                        data: b"hello"
                    }
                )
            };

            join!(f1, f2);
        })
    }

    #[test]
    fn send_interested_and_receive_unchoke() {
        block_on(async {
            let (a, b) = Peer::create_pair();
            let f1 = async move {
                let mut c = Client::new(a);
                assert!(c.conn.is_choked());
                c.send_interested();
                c.flush().await.unwrap();
                c.read_packet(&mut vec![]).await.unwrap();
                assert!(!c.conn.is_choked());
            };

            let f2 = async move {
                let mut c = Client::new(b);
                c.read_packet(&mut vec![]).await.unwrap();
            };

            join!(f1, f2);
        })
    }
    #[test]
    fn send_not_interested_and_receive_choke() {
        block_on(async {
            let (a, b) = Peer::create_pair();
            let f1 = async move {
                let mut c = Client::new(a);
                assert!(c.conn.is_choked());
                c.send_interested();
                c.flush().await.unwrap();
                c.read_packet(&mut vec![]).await.unwrap();
                assert!(!c.conn.is_choked());
                c.send_not_interested();
                c.flush().await.unwrap();
                c.read_packet(&mut vec![]).await.unwrap();
                assert!(c.conn.is_choked());
            };

            let f2 = async move {
                let mut c = Client::new(b);
                c.read_packet(&mut vec![]).await.unwrap();
                c.read_packet(&mut vec![]).await.unwrap();
            };

            join!(f1, f2);
        })
    }
}
