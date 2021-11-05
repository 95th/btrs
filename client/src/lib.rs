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

    pub async fn handshake(&mut self, info_hash: InfoHash, peer_id: PeerId) -> anyhow::Result<()> {
        let mut h = Handshake::new(info_hash, peer_id);
        h.set_extended(true);

        self.stream.write_all(h.as_bytes()).await?;
        self.stream.flush().await?;

        self.stream.read_exact(h.as_bytes_mut()).await?;

        ensure!(h.is_supported(), "Unsupported protocol");
        ensure!(h.info_hash == info_hash, "Incorrect infohash");

        Ok(())
    }

    pub async fn read_packet<'a>(
        &mut self,
        buf: &'a mut Vec<u8>,
    ) -> anyhow::Result<Option<Packet<'a>>> {
        let mut b = [0; 4];
        self.stream.read_exact(&mut b).await?;
        let len = u32::from_be_bytes(b);

        if len == 0 {
            // Keep-alive
            return Ok(None);
        }

        buf.resize(len as usize, 0);
        self.stream.read_exact(buf).await?;

        let header_len = Packet::header_size(buf[0]);
        ensure!(len as usize >= header_len + 1, "Invalid packet length");

        let packet = self.conn.read_packet(buf);
        self.flush().await?;
        Ok(packet)
    }

    pub async fn get_metadata(&mut self) -> anyhow::Result<Vec<u8>> {
        let buf = &mut Vec::new();
        loop {
            if let Some(Packet::Extended { data }) = self.read_packet(buf).await? {
                let ext = ExtendedMessage::parse(data, &mut self.parser)?;
                ensure!(ext.is_handshake(), "Expected extended handshake");

                let metadata = ext.metadata().context("Metadata extension not supported")?;

                self.conn
                    .send_extended(metadata.id, &MetadataMsg::Handshake(metadata.id));
                self.flush().await?;

                return self.read_metadata(metadata, buf).await;
            }
        }
    }

    async fn read_metadata(
        &mut self,
        metadata: Metadata,
        buf: &mut Vec<u8>,
    ) -> anyhow::Result<Vec<u8>> {
        let mut remaining = metadata.len;
        let mut piece = 0;
        let mut out_buf = Vec::new();

        while remaining > 0 {
            self.conn
                .send_extended(metadata.id, &MetadataMsg::Request(piece));
            self.flush().await?;

            let data = loop {
                if let Some(Packet::Extended { data }) = self.read_packet(buf).await? {
                    break data;
                }
            };

            let ext = ExtendedMessage::parse(data, &mut self.parser)?;
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

    pub async fn flush(&mut self) -> anyhow::Result<()> {
        let send_buf = self.conn.get_send_buf();
        if !send_buf.is_empty() {
            self.stream.write_all(&send_buf).await?;
        }
        Ok(())
    }
}
