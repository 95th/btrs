use crate::metainfo::InfoHash;
use crate::peer::PeerId;
use log::trace;
use std::convert::{TryFrom, TryInto};
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const PROTOCOL: &[u8] = b"BitTorrent protocol";

pub struct Handshake<'a> {
    pub extensions: [u8; 8],
    pub info_hash: &'a InfoHash,
    pub peer_id: &'a PeerId,
}

impl<'a> Handshake<'a> {
    const LEN: usize = 68;

    pub fn new(info_hash: &'a InfoHash, peer_id: &'a PeerId) -> Self {
        Self {
            peer_id,
            info_hash,
            extensions: Default::default(),
        }
    }

    pub fn set_extensions(&mut self, enable: bool) {
        if enable {
            self.extensions[5] |= 0x10;
        } else {
            self.extensions[5] &= !0x10;
        }
    }

    pub async fn write<W>(&self, writer: &mut W) -> io::Result<()>
    where
        W: AsyncWrite + Unpin,
    {
        trace!("Write handshake message");
        writer.write_u8(19).await?;
        writer.write_all(PROTOCOL).await?;
        writer.write_all(&self.extensions).await?;
        writer.write_all(self.info_hash.as_ref()).await?;
        writer.write_all(self.peer_id).await?;
        Ok(())
    }

    pub async fn read<R>(&self, reader: &mut R) -> crate::Result<PeerId>
    where
        R: AsyncRead + Unpin,
    {
        trace!("Read handshake message");
        let mut buf = [0; Handshake::LEN];
        reader.read_exact(&mut buf).await?;

        if buf[0] as usize != PROTOCOL.len() {
            return Err("Invalid length".into());
        }

        if &buf[1..20] != PROTOCOL {
            return Err("Invalid Protocol".into());
        }

        let info_hash = InfoHash::try_from(&buf[28..48])?;
        if self.info_hash != &info_hash {
            return Err("InfoHash mismatch".into());
        }

        let peer_id = buf[48..68].try_into().unwrap();
        Ok(peer_id)
    }
}
