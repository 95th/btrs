use anyhow::{ensure, Context};
use ben::Parser;
use bytes::BytesMut;
use futures::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use proto::{
    conn::Connection,
    ext::{ExtendedMessage, Metadata, MetadataMsg},
    handshake::Handshake,
    msg::Packet,
    InfoHash, PeerId,
};

pub struct Client<Stream> {
    stream: Stream,
    conn: Connection,
    recv_buf: BytesMut,
    parser: Parser,
}

impl<Stream> Client<Stream>
where
    Stream: AsyncRead + AsyncWrite + Unpin,
{
    pub async fn new(stream: Stream, info_hash: InfoHash, peer_id: PeerId) -> anyhow::Result<Self> {
        let mut client = Self {
            stream,
            conn: Connection::new(),
            recv_buf: BytesMut::with_capacity(1024),
            parser: Parser::new(),
        };

        client.handshake(info_hash, peer_id).await?;
        Ok(client)
    }

    async fn handshake(&mut self, info_hash: InfoHash, peer_id: PeerId) -> anyhow::Result<()> {
        let mut h = Handshake::new(info_hash, peer_id);
        h.set_extended(true);

        self.stream.write_all(h.as_bytes()).await?;
        self.stream.flush().await?;

        self.stream.read_exact(h.as_bytes_mut()).await?;

        ensure!(h.is_supported(), "Unsupported protocol");
        ensure!(h.info_hash == info_hash, "Incorrect infohash");

        Ok(())
    }

    pub async fn read_packet(&mut self) -> anyhow::Result<Packet> {
        let mut b = [0; 4];
        loop {
            self.stream.read_exact(&mut b).await?;
            let len = u32::from_be_bytes(b);

            if len == 0 {
                // Keep-alive
                continue;
            }

            self.recv_buf.resize(len as usize, 0);
            self.stream.read_exact(&mut self.recv_buf).await?;

            if let Some(packet) = self.conn.read_packet(&mut self.recv_buf) {
                return Ok(packet);
            }

            self.flush().await?;
        }
    }

    pub async fn get_metadata(&mut self) -> anyhow::Result<BytesMut> {
        loop {
            if let Packet::Extended { data } = self.read_packet().await? {
                let ext = ExtendedMessage::parse(&data, &mut self.parser)?;
                ensure!(ext.is_handshake(), "Expected extended handshake");

                let metadata = ext.metadata().context("Metadata extension not supported")?;

                self.conn
                    .send_extended(metadata.id, &MetadataMsg::Handshake(metadata.id));
                self.flush().await?;

                return self.read_metadata(metadata).await;
            }
        }
    }

    async fn read_metadata(&mut self, metadata: Metadata) -> anyhow::Result<BytesMut> {
        let mut remaining = metadata.len;
        let mut piece = 0;
        let mut buf = BytesMut::new();

        while remaining > 0 {
            self.conn
                .send_extended(metadata.id, &MetadataMsg::Request(piece));
            self.flush().await?;

            if let Packet::Extended { data } = self.read_packet().await? {
                let ext = ExtendedMessage::parse(&data, &mut self.parser)?;
                anyhow::ensure!(ext.id == metadata.id, "Expected Metadata message");

                let data = ext.data(piece)?;
                anyhow::ensure!(data.len() <= remaining, "Incorrect data length received");

                buf.extend_from_slice(data);
                remaining -= data.len();
                piece += 1;
            }
        }

        Ok(buf)
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

    pub async fn flush(&mut self) -> anyhow::Result<()> {
        let send_buf = self.conn.get_send_buf();
        self.stream.write_all(&send_buf).await?;
        Ok(())
    }
}
