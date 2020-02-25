use crate::client::AsyncStream;
use crate::metainfo::InfoHash;
use crate::peer::{Extensions, PeerId};
use log::trace;
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

impl<'a, C> Handshake<'a, C>
where
    C: AsyncStream,
{
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
            peer_id,
            info_hash,
            extensions,
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
        self.conn.write_all(&self.extensions).await?;
        self.conn.write_all(self.info_hash.as_ref()).await?;
        self.conn.write_all(&self.peer_id[..]).await?;
        self.conn.flush().await
    }

    pub async fn read(&mut self) -> crate::Result<HandshakeResult> {
        trace!("Read handshake message");

        let mut buf = [0; 68];
        self.conn.read_exact(&mut buf).await?;

        if !buf.starts_with(PROTOCOL) {
            return Err("Invalid Protocol".into());
        }

        let result = HandshakeResult {
            extensions: buf[20..28].try_into().unwrap(),
            info_hash: buf[28..48].try_into().unwrap(),
            peer_id: buf[48..68].try_into().unwrap(),
        };

        if &result.info_hash != self.info_hash {
            return Err("InfoHash mismatch".into());
        }

        Ok(result)
    }
}
