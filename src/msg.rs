use crate::util::read_u32;
use bencode::{Value, ValueRef};
use std::collections::BTreeMap;
use std::convert::TryFrom;
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

#[derive(Debug)]
pub struct Message {
    pub kind: MessageKind,
    pub payload: Vec<u8>,
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

    pub fn parse_extended(&self) -> Result<ExtendedMessage<'_>, &'static str> {
        if self.kind != MessageKind::Extended {
            return Err("Not an Extended message");
        }

        if self.payload.is_empty() {
            return Err("Extended message can't have empty payload");
        }

        ExtendedMessage::new(&self.payload)
    }
}

pub async fn write<W>(msg: Option<&Message>, writer: &mut W) -> crate::Result<()>
where
    W: AsyncWrite + Unpin,
{
    match msg {
        Some(msg) => msg.write(writer).await?,
        None => writer.write_u32(0).await?, // Keep-alive
    }
    Ok(())
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

pub fn extended_handshake() -> Message {
    Message {
        kind: MessageKind::Extended,
        payload: vec![0],
    }
}

pub fn extended(id: u8, data: &Value) -> Message {
    let mut payload = vec![id];
    data.encode(&mut payload).unwrap();
    Message {
        kind: MessageKind::Extended,
        payload,
    }
}

pub struct ExtendedMessage<'a> {
    pub id: u8,
    pub value: ValueRef<'a>,
    pub rest: &'a [u8],
}

impl ExtendedMessage<'_> {
    pub fn new(data: &[u8]) -> Result<ExtendedMessage, &'static str> {
        let id = data[0];
        let (value, i) = ValueRef::decode_prefix(&data[1..])
            .map_err(|_| "Invalid bencoded data in extended message")?;

        let rest = &data[i + 1..];
        Ok(ExtendedMessage { id, value, rest })
    }

    pub fn is_handshake(&self) -> bool {
        self.id == 0
    }

    pub fn metadata(&self) -> Option<Metadata> {
        let dict = self.value.as_dict()?;
        let m = dict.get("m")?.as_dict()?;
        let id = m.get("ut_metadata")?.as_int()? as u8;
        let len = dict.get("metadata_size")?.as_int()? as usize;
        Some(Metadata { id, len })
    }

    pub fn data(&self, expected_piece: usize) -> Result<&[u8], &'static str> {
        let dict = self.value.as_dict().ok_or("Not a dict")?;
        let msg_type = dict
            .get("msg_type")
            .and_then(|v| v.as_int())
            .ok_or("Msg type attr not found")?;

        if msg_type != 1 {
            return Err("Not a piece message");
        }

        let piece = dict
            .get("piece")
            .and_then(|v| v.as_int())
            .ok_or("Piece attr not found")? as usize;

        if piece != expected_piece {
            return Err("Not the right piece");
        }

        let total_size = dict
            .get("total_size")
            .and_then(|v| v.as_int())
            .ok_or("Total size attr not found")? as usize;

        if self.rest.len() != total_size {
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
    Request(usize),
}

impl MetadataMsg {
    pub fn as_value(&self) -> Value {
        match self {
            Self::Request(piece) => {
                let mut dict = BTreeMap::new();
                dict.insert("msg_type", Value::with_int(0));
                dict.insert("piece", Value::with_int(*piece as i64));
                Value::with_dict(dict)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn extended_new() {
        let ext = ExtendedMessage::new(&[0, b'd', b'e', 1, 2, 3, 4]).unwrap();
        assert_eq!(0, ext.id);
        assert_eq!(ValueRef::with_dict(BTreeMap::new()), ext.value);
        assert_eq!(&[1, 2, 3, 4], ext.rest);
    }
}
