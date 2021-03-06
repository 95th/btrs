use anyhow::Context;
use ben::{Decoder, Encode, Encoder, Parser};
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const METADATA_PIECE_LEN: usize = 0x4000;

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
            Unknown { .. } => {
                debug_assert!(false, "Can't be here");
                u8::max_value()
            }
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
                writer.write_u32(data.len() as u32 + 9).await?;
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
        if let Extended { .. } = self {
            writer.write_u32(data.len() as u32 + 2).await?;
            writer.write_u8(self.type_id()).await?;
            writer.write_u8(id).await?;
            writer.write_all(data).await?;
        }
        Ok(())
    }

    pub async fn read<R>(reader: &mut R) -> crate::Result<Option<Self>>
    where
        R: AsyncRead + Unpin,
    {
        use Message::*;
        let len = reader.read_u32().await?;
        if len == 0 {
            // Keep-alive
            return Ok(None);
        }

        let id = reader.read_u8().await?;
        trace!("got id: {}", id);

        let msg = match id {
            0 => {
                ensure!(len == 1, "Invalid Choke");
                Choke
            }
            1 => {
                ensure!(len == 1, "Invalid Unchoke");
                Unchoke
            }
            2 => {
                ensure!(len == 1, "Invalid Interested");
                Interested
            }
            3 => {
                ensure!(len == 1, "Invalid NotInterested");
                NotInterested
            }
            4 => {
                ensure!(len == 5, "Invalid Have");
                Have {
                    index: reader.read_u32().await?,
                }
            }
            5 => Bitfield { len: len - 1 },
            6 => {
                ensure!(len == 13, "Invalid Request");
                let index = reader.read_u32().await?;
                let begin = reader.read_u32().await?;
                let len = reader.read_u32().await?;
                Request { index, begin, len }
            }
            7 => {
                ensure!(len > 9, "Invalid Piece");
                let index = reader.read_u32().await?;
                let begin = reader.read_u32().await?;
                Piece {
                    index,
                    begin,
                    len: len - 9,
                }
            }
            8 => {
                ensure!(len == 13, "Invalid Cancel");
                let index = reader.read_u32().await?;
                let begin = reader.read_u32().await?;
                let len = reader.read_u32().await?;
                Cancel { index, begin, len }
            }
            20 => Extended { len: len - 1 },
            id => Unknown { id, len: len - 1 },
        };

        Ok(Some(msg))
    }

    pub async fn read_discard<R>(&self, rdr: &mut R) -> io::Result<()>
    where
        R: AsyncRead + Unpin,
    {
        use Message::*;
        trace!("read_discard: {:?}", self);
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

    pub async fn read_piece<R>(&self, rdr: &mut R, buf: &mut [u8]) -> crate::Result<()>
    where
        R: AsyncRead + Unpin,
    {
        match *self {
            Message::Piece { begin, len, .. } => {
                let begin = begin as usize;
                ensure!(begin <= buf.len(), "Begin offset too high");

                let len = len as usize;
                trace!("Reading piece message of len: {}", len);

                ensure!(begin + len <= buf.len(), "Data too large");

                rdr.read_exact(&mut buf[begin..][..len]).await?;
                Ok(())
            }
            _ => bail!("Not a piece"),
        }
    }

    pub async fn read_bitfield<R>(&self, rdr: &mut R, buf: &mut [u8]) -> crate::Result<()>
    where
        R: AsyncRead + Unpin,
    {
        match *self {
            Message::Bitfield { len } => {
                let len = len as usize;
                trace!("Reading bitfield message of len: {}", len);
                ensure!(len <= buf.len(), "Data too large");

                rdr.read_exact(&mut buf[..len]).await?;
                Ok(())
            }
            _ => bail!("Not a piece"),
        }
    }

    pub async fn read_ext<'a, 'p, R>(
        &self,
        rdr: &mut R,
        buf: &'a mut Vec<u8>,
        parser: &'p mut Parser,
    ) -> crate::Result<ExtendedMessage<'a, 'p>>
    where
        R: AsyncRead + Unpin,
    {
        match *self {
            Self::Extended { len } => {
                let len = len as usize;
                trace!("Reading ext message of len: {}", len);
                buf.clear();
                buf.resize(len as usize, 0);
                rdr.read_exact(buf).await?;
                let msg = ExtendedMessage::parse(buf, parser)?;
                Ok(msg)
            }
            _ => bail!("Not an Extended message"),
        }
    }
}

pub struct ExtendedMessage<'a, 'p> {
    pub id: u8,
    pub value: Decoder<'a, 'p>,
    pub rest: &'a [u8],
}

mod msg_type {
    pub const REQUEST: i64 = 0;
    pub const DATA: i64 = 1;
    pub const REJECT: i64 = 2;
}

impl<'a, 'p> ExtendedMessage<'a, 'p> {
    pub fn parse(data: &'a [u8], parser: &'p mut Parser) -> crate::Result<Self> {
        let id = data[0];
        let (value, i) = parser.parse_prefix::<Decoder>(&data[1..])?;
        debug!("ext header len: {}", value.as_raw_bytes().len());

        let rest = &data[i + 1..];
        debug!("ext data len: {}", rest.len());
        Ok(Self { id, value, rest })
    }

    pub fn is_handshake(&self) -> bool {
        self.id == 0
    }

    pub fn body(&self) -> &Decoder<'_, '_> {
        &self.value
    }

    pub fn metadata(&self) -> Option<Metadata> {
        trace!("metadata: {:#?}", self.value);
        let dict = self.value.as_dict()?;
        let m = dict.get_dict("m")?;
        let id = m.get_int("ut_metadata")? as u8;
        let len = dict.get_int("metadata_size")? as usize;
        Some(Metadata { id, len })
    }

    pub fn data(&self, expected_piece: i64) -> crate::Result<&[u8]> {
        trace!("data: {:#?}", self.value);
        let dict = self.value.as_dict().context("Not a dict")?;

        let msg_type = dict.get_int("msg_type").context("`msg_type` not found")?;
        ensure!(msg_type == msg_type::DATA, "Not a DATA message");

        let piece = dict.get_int("piece").context("`piece` not found")?;
        ensure!(piece == expected_piece, "Incorrect piece");

        if self.rest.len() > METADATA_PIECE_LEN {
            bail!("Piece can't be larger than 16kB");
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
    Handshake(u8),
    Request(i64),
    Reject(i64),
    Data(i64, i64),
}

impl Encode for MetadataMsg {
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        let mut dict = encoder.add_dict();
        match *self {
            MetadataMsg::Handshake(id) => {
                let mut m = dict.add_dict("m");
                m.add("ut_metadata", i64::from(id));
                m.finish();

                dict.add("p", 6881);
                dict.add("reqq", 500);
            }
            MetadataMsg::Request(piece) => {
                dict.add("msg_type", msg_type::REQUEST);
                dict.add("piece", piece);
            }
            MetadataMsg::Reject(piece) => {
                dict.add("msg_type", msg_type::REJECT);
                dict.add("piece", piece);
            }
            MetadataMsg::Data(piece, total_size) => {
                dict.add("msg_type", msg_type::DATA);
                dict.add("piece", piece);
                dict.add("total_size", total_size);
            }
        }
        dict.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn extended_new() {
        let mut parser = Parser::new();
        let ext = ExtendedMessage::parse(&[0, b'd', b'e', 1, 2, 3, 4], &mut parser).unwrap();
        assert_eq!(0, ext.id);
        assert!(ext.value.is_dict());
        assert_eq!(b"de", ext.value.as_raw_bytes());
        assert_eq!(&[1, 2, 3, 4], ext.rest);
    }

    #[test]
    fn extended_new_2() {
        let mut parser = Parser::new();
        let ext = ExtendedMessage::parse(&[0, b'd', b'e'], &mut parser).unwrap();
        assert_eq!(0, ext.id);
        assert!(ext.value.is_dict());
        assert_eq!(b"de", ext.value.as_raw_bytes());
        assert!(ext.rest.is_empty());
    }

    #[tokio::test]
    async fn read_discard_piece() {
        let v = [
            0, 0, 0, 12, 7, 0x01, 0x02, 0x03, 0x04, 0x04, 0x03, 0x02, 0x01, 1, 2, 3,
        ];
        let mut c = Cursor::new(&v);
        let m = Message::read(&mut c).await.unwrap().unwrap();
        assert_eq!(
            Message::Piece {
                index: 0x0102_0304,
                begin: 0x0403_0201,
                len: 3
            },
            m
        );
        m.read_discard(&mut c).await.unwrap();
        assert_eq!(v.len(), c.position() as usize);
    }

    #[tokio::test]
    async fn read_choke() {
        let v = [0, 0, 0, 1, 0];
        let mut c = Cursor::new(&v);
        let m = Message::read(&mut c).await.unwrap().unwrap();
        assert_eq!(Message::Choke, m);
        assert_eq!(v.len(), c.position() as usize);
    }

    #[tokio::test]
    async fn read_unchoke() {
        let v = [0, 0, 0, 1, 1];
        let mut c = Cursor::new(&v);
        let m = Message::read(&mut c).await.unwrap().unwrap();
        assert_eq!(Message::Unchoke, m);
        assert_eq!(v.len(), c.position() as usize);
    }

    #[tokio::test]
    async fn read_interested() {
        let v = [0, 0, 0, 1, 2];
        let mut c = Cursor::new(&v);
        let m = Message::read(&mut c).await.unwrap().unwrap();
        assert_eq!(Message::Interested, m);
        assert_eq!(v.len(), c.position() as usize);
    }

    #[tokio::test]
    async fn read_not_interested() {
        let v = [0, 0, 0, 1, 3];
        let mut c = Cursor::new(&v);
        let m = Message::read(&mut c).await.unwrap().unwrap();
        assert_eq!(Message::NotInterested, m);
        assert_eq!(v.len(), c.position() as usize);
    }

    #[tokio::test]
    async fn read_have() {
        let v = [0, 0, 0, 5, 4, 0x01, 0x02, 0x03, 0x04];
        let mut c = Cursor::new(&v);
        let m = Message::read(&mut c).await.unwrap().unwrap();
        assert_eq!(Message::Have { index: 0x0102_0304 }, m);
        assert_eq!(v.len(), c.position() as usize);
    }

    #[tokio::test]
    async fn read_bitfield() {
        let v = [0, 0, 0, 5, 5, 0x01, 0x02, 0x03, 0x04];
        let mut c = Cursor::new(&v);
        let m = Message::read(&mut c).await.unwrap().unwrap();
        assert_eq!(Message::Bitfield { len: 4 }, m);
        let mut buf = [0; 4];
        m.read_bitfield(&mut c, &mut buf).await.unwrap();
        assert_eq!([0x01, 0x02, 0x03, 0x04], buf);
        assert_eq!(v.len(), c.position() as usize);
    }

    #[tokio::test]
    async fn read_request() {
        let v = [
            0, 0, 0, 13, 6, 0x01, 0x02, 0x03, 0x04, 0x01, 0x02, 0x03, 0x04, 0x01, 0x02, 0x03, 0x04,
        ];
        let mut c = Cursor::new(&v);
        let m = Message::read(&mut c).await.unwrap().unwrap();
        assert_eq!(
            Message::Request {
                index: 0x0102_0304,
                begin: 0x0102_0304,
                len: 0x0102_0304,
            },
            m
        );
        assert_eq!(v.len(), c.position() as usize);
    }

    #[tokio::test]
    async fn read_piece() {
        let v = [0, 0, 0, 12, 7, 0x01, 0x02, 0x03, 0x04, 0, 0, 0, 0, 1, 2, 3];
        let mut c = Cursor::new(&v);
        let m = Message::read(&mut c).await.unwrap().unwrap();
        assert_eq!(
            Message::Piece {
                index: 0x0102_0304,
                begin: 0,
                len: 3
            },
            m
        );
        let mut d = [0; 3];
        m.read_piece(&mut c, &mut d).await.unwrap();
        assert_eq!(&[1, 2, 3], &d[..]);
        assert_eq!(v.len(), c.position() as usize);
    }

    #[tokio::test]
    async fn read_cancel() {
        let v = [
            0, 0, 0, 13, 8, 0x01, 0x02, 0x03, 0x04, 0x01, 0x02, 0x03, 0x04, 0x01, 0x02, 0x03, 0x04,
        ];
        let mut c = Cursor::new(&v);
        let m = Message::read(&mut c).await.unwrap().unwrap();
        assert_eq!(
            Message::Cancel {
                index: 0x0102_0304,
                begin: 0x0102_0304,
                len: 0x0102_0304,
            },
            m
        );
        assert_eq!(v.len(), c.position() as usize);
    }

    #[tokio::test]
    async fn read_extended() {
        let v = [0, 0, 0, 4, 20, 1, b'd', b'e'];
        let mut c = Cursor::new(&v);
        let m = Message::read(&mut c).await.unwrap().unwrap();
        assert_eq!(Message::Extended { len: 3 }, m);
        let mut buf = vec![0; 3];
        let mut parser = Parser::new();
        let ext = m.read_ext(&mut c, &mut buf, &mut parser).await.unwrap();
        assert!(!ext.is_handshake());
        assert!(ext.metadata().is_none());
        assert_eq!(v.len(), c.position() as usize);
    }
}
