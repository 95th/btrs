use crate::client::AsyncStream;
use crate::metainfo::InfoHash;
use crate::peer::{Extensions, PeerId};
use std::convert::TryInto;
use std::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const PROTOCOL: &[u8] = b"\x13BitTorrent protocol";

pub struct Handshake<'a, C> {
    pub conn: &'a mut C,
    pub extensions: Extensions,
    pub info_hash: &'a InfoHash,
    pub peer_id: &'a PeerId,
}

#[derive(Debug)]
pub struct HandshakeResult {
    pub extensions: Extensions,
    pub info_hash: InfoHash,
    pub peer_id: PeerId,
}

impl<'a, C: AsyncStream> Handshake<'a, C> {
    pub fn new(conn: &'a mut C, info_hash: &'a InfoHash, peer_id: &'a PeerId) -> Self {
        Self::with_extensions(conn, info_hash, peer_id, Default::default())
    }

    pub fn with_extensions(
        conn: &'a mut C,
        info_hash: &'a InfoHash,
        peer_id: &'a PeerId,
        extensions: Extensions,
    ) -> Self {
        Self {
            conn,
            extensions,
            info_hash,
            peer_id,
        }
    }

    pub fn set_extended(&mut self, enable: bool) {
        if enable {
            self.extensions[5] |= 0x10;
        } else {
            self.extensions[5] &= !0x10;
        }
    }

    pub async fn write(&mut self) -> io::Result<()> {
        trace!("Write handshake message");
        self.conn.write_all(PROTOCOL).await?;
        self.conn.write_all(&self.extensions[..]).await?;
        self.conn.write_all(self.info_hash.as_ref()).await?;
        self.conn.write_all(&self.peer_id[..]).await?;
        self.conn.flush().await
    }

    pub async fn read(&mut self) -> anyhow::Result<HandshakeResult> {
        trace!("Read handshake message");

        let mut buf = [0; 68];
        self.conn.read_exact(&mut buf).await?;

        anyhow::ensure!(buf.starts_with(PROTOCOL), "Invalid Protocol");

        let result = HandshakeResult {
            extensions: Extensions::new(buf[20..28].try_into().unwrap()),
            info_hash: buf[28..48].try_into().unwrap(),
            peer_id: buf[48..68].try_into().unwrap(),
        };

        anyhow::ensure!(*self.info_hash == result.info_hash, "InfoHash mismatch");
        Ok(result)
    }
}
