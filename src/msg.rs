use crate::util::read_u32;
use bencode::{Value, ValueRef};
use std::convert::TryFrom;
use tokio::io::{AsyncRead, AsyncWrite};

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
        use tokio::io::AsyncWriteExt;
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

        let id = self.payload[0];
        Ok(ExtendedMessage {
            id,
            payload: &self.payload[1..],
        })
    }
}

pub async fn write<W>(msg: Option<&Message>, writer: &mut W) -> crate::Result<()>
where
    W: AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;
    match msg {
        Some(msg) => msg.write(writer).await?,
        // Keep-alive
        None => writer.write_all(&[0; 4]).await?,
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

#[derive(Copy, Clone)]
#[repr(u8)]
pub enum MetadataMsgKind {
    Request = 0,
    Data = 1,
    Reject = 2,
}

pub struct ExtendedMessage<'a> {
    pub id: u8,
    pub payload: &'a [u8],
}

impl<'a> ExtendedMessage<'a> {
    pub fn is_handshake(&self) -> bool {
        self.id == 0
    }

    pub fn parse(&self) -> Result<ValueRef<'a>, &'static str> {
        ValueRef::decode_prefix(&self.payload)
            .map(|(v, _)| v)
            .map_err(|_| "Invalid bencoded data in Extended message")
    }
}
