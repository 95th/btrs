use ben::{Entry, Node};
use log::debug;
use log::trace;
use std::collections::BTreeMap;
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const METADATA_PIECE_LEN: usize = 16384;

#[derive(Debug, PartialEq)]
pub enum Message {
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have { index: u32 },
    Bitfield { len: u32 },
    Request { index: u32, begin: u32, len: u32 },
    Piece { index: u32, begin: u32, len: u32 },
    Cancel { index: u32, begin: u32, len: u32 },
    Extended { len: u32 },
    Unknown { id: u8, len: u32 },
}

impl Message {
    pub fn type_id(&self) -> u8 {
        use Message::*;
        match self {
            Choke => 0,
            Unchoke => 1,
            Interested => 2,
            NotInterested => 3,
            Have { .. } => 4,
            Bitfield { .. } => 5,
            Request { .. } => 6,
            Piece { .. } => 7,
            Cancel { .. } => 8,
            Extended { .. } => 20,
            Unknown { .. } => panic!("Not sendable"),
        }
    }

    pub async fn write<W>(&self, writer: &mut W) -> io::Result<()>
    where
        W: AsyncWrite + Unpin,
    {
        use Message::*;

        match *self {
            Choke | Unchoke | Interested | NotInterested | Extended { len: 0 } => {
                writer.write_u32(1).await?;
                writer.write_u8(self.type_id()).await?;
            }
            Have { index } => {
                writer.write_u32(5).await?;
                writer.write_u8(self.type_id()).await?;
                writer.write_u32(index).await?;
            }
            Request { index, begin, len } | Cancel { index, begin, len } => {
                writer.write_u32(13).await?;
                writer.write_u8(self.type_id()).await?;
                writer.write_u32(index).await?;
                writer.write_u32(begin).await?;
                writer.write_u32(len).await?;
            }
            _ => {}
        }
        Ok(())
    }

    pub async fn write_buf<W>(&self, writer: &mut W, data: &[u8]) -> io::Result<()>
    where
        W: AsyncWrite + Unpin,
    {
        use Message::*;

        match *self {
            Bitfield { .. } => {
                writer.write_u32(data.len() as u32 + 1).await?;
                writer.write_u8(self.type_id()).await?;
                writer.write_all(data).await?;
            }
            Piece { index, begin, .. } => {
                writer.write_u32(data.len() as u32 + 13).await?;
                writer.write_u8(self.type_id()).await?;
                writer.write_u32(index).await?;
                writer.write_u32(begin).await?;
                writer.write_all(data).await?
            }
            _ => {}
        }
        Ok(())
    }

    pub async fn write_ext<W>(&self, writer: &mut W, id: u8, data: &[u8]) -> io::Result<()>
    where
        W: AsyncWrite + Unpin,
    {
        use Message::*;
        if let Bitfield { .. } = self {
            writer.write_u32(data.len() as u32 + 2).await?;
            writer.write_u8(self.type_id()).await?;
            writer.write_u8(id).await?;
            writer.write_all(&data).await?;
        }
        Ok(())
    }

    pub async fn read<R>(reader: &mut R) -> crate::Result<Option<Self>>
    where
        R: AsyncRead + Unpin,
    {
        let mut len = reader.read_u32().await?;
        if len == 0 {
            // Keep-alive
            return Ok(None);
        }
        len -= 1;

        let id = reader.read_u8().await?;
        debug!("got id: {}", id);

        let msg = match id {
            0 => Self::Choke,
            1 => Self::Unchoke,
            2 => Self::Interested,
            3 => Self::NotInterested,
            4 => Self::Have {
                index: reader.read_u32().await?,
            },
            5 => Self::Bitfield { len },
            6 => Self::Request {
                index: reader.read_u32().await?,
                begin: reader.read_u32().await?,
                len: reader.read_u32().await?,
            },
            7 => {
                if len <= 8 {
                    return Err("Invalid Piece length".into());
                }
                Self::Piece {
                    index: reader.read_u32().await?,
                    begin: reader.read_u32().await?,
                    len: len - 8,
                }
            }
            8 => Self::Cancel {
                index: reader.read_u32().await?,
                begin: reader.read_u32().await?,
                len: reader.read_u32().await?,
            },
            20 => Self::Extended { len },
            id => Self::Unknown { id, len },
        };

        Ok(Some(msg))
    }

    pub async fn read_discard<R>(&self, rdr: &mut R) -> io::Result<()>
    where
        R: AsyncRead + Unpin,
    {
        use Message::*;
        trace!("read_consume");
        if let Piece { len, .. } | Bitfield { len } | Extended { len } | Unknown { len, .. } = *self
        {
            let total = len as usize;
            trace!("have to ignore {} bytes", total);
            let mut done = 0;
            let mut buf = [0u8; 1024];
            while done < total {
                let n = (total - done).min(1024);
                trace!("Reading ignore bytes: {}", n);
                rdr.read_exact(&mut buf[..n]).await?;
                done += n;
            }
        }
        Ok(())
    }

    pub async fn read_piece<R>(
        &self,
        request_idx: u32,
        rdr: &mut R,
        buf: &mut [u8],
    ) -> crate::Result<()>
    where
        R: AsyncRead + Unpin,
    {
        match *self {
            Message::Piece { index, begin, len } => {
                if request_idx != index {
                    return Err("Piece Index mismatch".into());
                }

                let begin = begin as usize;
                if begin > buf.len() {
                    return Err("Begin offset too high".into());
                }

                let len = len as usize;
                debug!("Reading piece message of len: {}", len);
                if begin + len > buf.len() {
                    return Err("Data too large".into());
                }

                rdr.read_exact(&mut buf[begin..][..len]).await?;
                Ok(())
            }
            _ => Err("Not a piece".into()),
        }
    }

    pub async fn read_bitfield<R>(&self, rdr: &mut R, buf: &mut [u8]) -> crate::Result<()>
    where
        R: AsyncRead + Unpin,
    {
        match *self {
            Message::Bitfield { len } => {
                let len = len as usize;
                debug!("Reading bitfield message of len: {}", len);
                if len > buf.len() {
                    return Err("Data too large".into());
                }

                rdr.read_exact(&mut buf[..len]).await?;
                Ok(())
            }
            _ => Err("Not a piece".into()),
        }
    }

    pub async fn read_ext<'a, R>(
        &self,
        rdr: &mut R,
        buf: &'a mut Vec<u8>,
    ) -> crate::Result<ExtendedMessage<'a>>
    where
        R: AsyncRead + Unpin,
    {
        match *self {
            Self::Extended { len } => {
                let len = len as usize;
                debug!("Reading ext message of len: {}", len);
                if len > buf.len() {
                    return Err("Data too large".into());
                }

                buf.resize(len as usize, 0);
                rdr.read_exact(buf).await?;
                let msg = ExtendedMessage::new(buf)?;
                Ok(msg)
            }
            _ => Err("Not an Extended message".into()),
        }
    }
}

pub struct ExtendedMessage<'a> {
    pub id: u8,
    pub value: Node<'a>,
    pub rest: &'a [u8],
}

mod msg_type {
    pub const REQUEST: i64 = 1;
    pub const DATA: i64 = 1;
    pub const REJECT: i64 = 2;
}

impl ExtendedMessage<'_> {
    pub fn new(data: &[u8]) -> Result<ExtendedMessage, &'static str> {
        let id = data[0];
        let (value, i) = Node::parse_prefix(&data[1..])
            .map_err(|_| "Invalid bencoded data in extended message")?;

        let rest = &data[i + 1..];
        Ok(ExtendedMessage { id, value, rest })
    }

    pub fn is_handshake(&self) -> bool {
        self.id == 0
    }

    pub fn metadata(&self) -> Option<Metadata> {
        let dict = self.value.as_dict()?;
        let m = dict.get_dict(b"m")?;
        let id = m.get_int(b"ut_metadata")? as u8;
        let len = dict.get_int(b"metadata_size")? as usize;
        Some(Metadata { id, len })
    }

    pub fn data(&self, expected_piece: i64) -> Result<&[u8], &'static str> {
        let dict = self.value.as_dict().ok_or("Not a dict")?;

        let msg_type = dict.get_int(b"msg_type").ok_or("msg_type is not int")?;
        if msg_type != msg_type::DATA {
            return Err("Not a DATA message");
        }

        let piece = dict.get_int(b"piece").ok_or("piece is not int")?;
        if piece != expected_piece {
            return Err("Not the right piece");
        }

        let total_size = dict.get_int(b"total_size").ok_or("total_size is not int")?;
        if self.rest.len() as i64 != total_size {
            return Err("Incorrect size");
        }

        if self.rest.len() > METADATA_PIECE_LEN {
            return Err("Piece can't be larger than 16kB");
        }

        Ok(self.rest)
    }
}

#[derive(Debug)]
pub struct Metadata {
    pub id: u8,
    pub len: usize,
}

pub enum MetadataMsg {
    Request(i64),
    Reject(i64),
    Data(i64, i64),
}

impl From<MetadataMsg> for Entry {
    fn from(msg: MetadataMsg) -> Self {
        let mut dict = BTreeMap::new();
        match msg {
            MetadataMsg::Request(piece) => {
                dict.insert("msg_type", msg_type::REQUEST.into());
                dict.insert("piece", piece.into());
            }
            MetadataMsg::Reject(piece) => {
                dict.insert("msg_type", msg_type::REJECT.into());
                dict.insert("piece", piece.into());
            }
            MetadataMsg::Data(piece, total_size) => {
                dict.insert("msg_type", msg_type::DATA.into());
                dict.insert("piece", piece.into());
                dict.insert("total_size", total_size.into());
            }
        }
        dict.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn extended_new() {
        let ext = ExtendedMessage::new(&[0, b'd', b'e', 1, 2, 3, 4]).unwrap();
        assert_eq!(0, ext.id);
        assert!(ext.value.is_dict());
        assert_eq!(b"de", ext.value.data());
        assert_eq!(&[1, 2, 3, 4], ext.rest);
    }

    #[tokio::test]
    async fn read_piece() {
        let v = [0, 0, 0, 12, 7, 0, 0, 0, 1, 0, 0, 0, 0, 1, 2, 3];
        let mut c = Cursor::new(&v);
        let m = Message::read(&mut c).await.unwrap().unwrap();
        assert_eq!(
            Message::Piece {
                index: 1,
                begin: 0,
                len: 3
            },
            m
        );
        let mut d = [0; 3];
        m.read_piece(1, &mut c, &mut d).await.unwrap();
        assert_eq!(&[1, 2, 3], &d[..]);
    }

    #[tokio::test]
    async fn read_discard_piece() {
        let v = [0, 0, 0, 12, 7, 0, 0, 0, 1, 0, 0, 0, 0, 1, 2, 3];
        let mut c = Cursor::new(&v);
        let m = Message::read(&mut c).await.unwrap().unwrap();
        assert_eq!(
            Message::Piece {
                index: 1,
                begin: 0,
                len: 3
            },
            m
        );
        m.read_discard(&mut c).await.unwrap();
        let n = c.read(&mut [0]).await.unwrap();
        assert_eq!(0, n);
    }
}
