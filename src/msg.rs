use crate::bitfield::BitField;
use crate::util::read_u32;
use ben::{Node, WriteNode};
use log::trace;
use std::collections::BTreeMap;
use std::convert::TryFrom;
use std::fmt;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

const METADATA_PIECE_LEN: usize = 16384;

#[derive(Debug, Copy, Clone, PartialEq)]
#[repr(u8)]
pub enum MessageKind {
    Choke = 0,
    Unchoke = 1,
    Interested = 2,
    NotInterested = 3,
    Have = 4,
    Bitfield = 5,
    Request = 6,
    Piece = 7,
    Cancel = 8,
    Extended = 20,
}

impl TryFrom<u8> for MessageKind {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        use MessageKind::*;

        Ok(match value {
            0 => Choke,
            1 => Unchoke,
            2 => Interested,
            3 => NotInterested,
            4 => Have,
            5 => Bitfield,
            6 => Request,
            7 => Piece,
            8 => Cancel,
            20 => Extended,
            _ => return Err("Invalid Message Kind"),
        })
    }
}

pub struct Message {
    pub kind: MessageKind,
    pub payload: Vec<u8>,
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Message")
            .field("kind", &self.kind)
            .field("payload", &"..")
            .finish()
    }
}

impl Message {
    pub fn new(kind: MessageKind, payload: Vec<u8>) -> Self {
        Self { kind, payload }
    }

    pub async fn write<W>(&self, writer: &mut W) -> crate::Result<()>
    where
        W: AsyncWrite + Unpin,
    {
        let len = self.payload.len() as u32 + 1; // +1 for MessageKind
        writer.write_u32(len).await?;
        writer.write_u8(self.kind as u8).await?;
        writer.write_all(&self.payload).await?;
        Ok(())
    }

    pub fn parse_piece(&self, index: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        if self.kind != MessageKind::Piece {
            return Err("Not a Piece message");
        }

        if self.payload.len() < 8 {
            return Err("Message too short");
        }

        let parsed_idx = read_u32(&self.payload[..4]) as usize;
        if parsed_idx != index {
            return Err("Piece Index mismatch");
        }

        let begin = read_u32(&self.payload[4..8]) as usize;
        if begin >= buf.len() {
            return Err("Begin offset too high");
        }

        let data = &self.payload[8..];
        if begin + data.len() > buf.len() {
            return Err("Data too large");
        }

        buf[begin..][..data.len()].copy_from_slice(data);
        Ok(data.len())
    }

    pub fn parse_have(&self) -> Result<usize, &'static str> {
        if self.kind != MessageKind::Have {
            return Err("Not a Have message");
        }

        if self.payload.len() != 4 {
            return Err("Message has incorrect length payload");
        }

        let index = read_u32(&self.payload) as usize;
        Ok(index)
    }

    pub fn parse_bitfield(self) -> Result<BitField, &'static str> {
        if self.kind != MessageKind::Bitfield {
            return Err("Not a bitfield message");
        }

        Ok(self.payload.into())
    }

    pub fn parse_ext(&self) -> Result<ExtendedMessage<'_>, &'static str> {
        if self.kind != MessageKind::Extended {
            trace!("Expected extended msg, got {:?}", self.kind);
            return Err("Not an Extended message");
        }

        if self.payload.is_empty() {
            return Err("Extended message can't have empty payload");
        }

        ExtendedMessage::new(&self.payload)
    }
}

pub async fn read<R>(reader: &mut R) -> crate::Result<Option<Message>>
where
    R: AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;
    let len = reader.read_u32().await?;
    if len == 0 {
        // Keep-alive
        return Ok(None);
    }

    let b = reader.read_u8().await?;
    let kind = MessageKind::try_from(b)?;

    let payload = if len == 1 {
        vec![]
    } else {
        let mut payload = vec![0; (len - 1) as usize];
        reader.read_exact(&mut payload).await?;
        payload
    };

    Ok(Some(Message { kind, payload }))
}

pub fn request(index: u32, begin: u32, length: u32) -> Message {
    let mut payload = vec![0; 12];
    payload[..4].copy_from_slice(&index.to_be_bytes());
    payload[4..8].copy_from_slice(&begin.to_be_bytes());
    payload[8..].copy_from_slice(&length.to_be_bytes());
    Message::new(MessageKind::Request, payload)
}

pub fn interested() -> Message {
    Message::new(MessageKind::Interested, vec![])
}

pub fn not_interested() -> Message {
    Message::new(MessageKind::NotInterested, vec![])
}

pub fn choke() -> Message {
    Message::new(MessageKind::Choke, vec![])
}

pub fn unchoke() -> Message {
    Message::new(MessageKind::Unchoke, vec![])
}

pub fn have(index: u32) -> Message {
    Message::new(MessageKind::Have, index.to_be_bytes().to_vec())
}

pub fn ext_handshake() -> Message {
    Message {
        kind: MessageKind::Extended,
        payload: vec![0],
    }
}

pub fn ext(id: u8, data: &WriteNode) -> Message {
    let mut payload = vec![id];
    data.write(&mut payload).unwrap();
    Message {
        kind: MessageKind::Extended,
        payload,
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

impl MetadataMsg {
    pub fn as_value(&self) -> WriteNode {
        let mut dict = BTreeMap::new();
        match *self {
            Self::Request(piece) => {
                dict.insert("msg_type", msg_type::REQUEST.into());
                dict.insert("piece", piece.into());
            }
            Self::Reject(piece) => {
                dict.insert("msg_type", msg_type::REJECT.into());
                dict.insert("piece", piece.into());
            }
            Self::Data(piece, total_size) => {
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

    #[test]
    fn extended_new() {
        let ext = ExtendedMessage::new(&[0, b'd', b'e', 1, 2, 3, 4]).unwrap();
        assert_eq!(0, ext.id);
        assert!(ext.value.is_dict());
        assert_eq!(b"", ext.value.data());
        assert_eq!(&[1, 2, 3, 4], ext.rest);
    }
}
