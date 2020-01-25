use std::convert::TryFrom;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Copy, Clone)]
#[repr(u8)]
pub enum MessageKind {
    Choke = 0,
    Unchoke,
    Interested,
    NotInterested,
    Have,
    Bitfield,
    Request,
    Piece,
    Cancel,
}

impl TryFrom<u8> for MessageKind {
    type Error = crate::Error;

    fn try_from(value: u8) -> crate::Result<Self> {
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
            _ => Err("Invalid Message Kind")?,
        })
    }
}

pub struct Message {
    kind: MessageKind,
    payload: Vec<u8>,
}

impl Message {
    pub fn new(kind: MessageKind, payload: Vec<u8>) -> Self {
        Self { kind, payload }
    }

    pub fn kind(&self) -> MessageKind {
        self.kind
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
}

pub async fn write<W>(msg: Option<&Message>, writer: &mut W) -> crate::Result<()>
where
    W: AsyncWrite + Unpin,
{
    match msg {
        Some(msg) => {
            let len = msg.payload.len() as u32 + 1; // +1 for MessageKind
            writer.write_all(&len.to_be_bytes()).await?;
            writer.write_all(&[msg.kind as u8]).await?;
            writer.write_all(&msg.payload).await?;
        }
        // Keep-alive
        None => writer.write_all(&[0; 4]).await?,
    }
    Ok(())
}

pub async fn read<R>(reader: &mut R) -> crate::Result<Option<Message>>
where
    R: AsyncRead + Unpin,
{
    let mut buf = [0; 4];
    reader.read_exact(&mut buf[..4]).await?;
    let len = u32::from_be_bytes(buf);
    if len == 0 {
        // Keep-alive
        return Ok(None);
    }

    reader.read_exact(&mut buf[..1]).await?;
    let kind = MessageKind::try_from(buf[0])?;

    let payload = if len == 1 {
        vec![]
    } else {
        let mut payload = vec![0; (len - 1) as usize];
        reader.read_exact(&mut payload).await?;
        payload
    };

    Ok(Some(Message { kind, payload }))
}
