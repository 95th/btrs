use std::io;

use futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub trait AsyncStream: AsyncRead + AsyncWrite + Unpin {}

impl<T: AsyncRead + AsyncWrite + Unpin> AsyncStream for T {}

pub async fn write_u32<W: AsyncWrite + Unpin>(writer: &mut W, value: u32) -> io::Result<()> {
    writer.write_all(&value.to_be_bytes()).await
}

pub async fn write_u8<W: AsyncWrite + Unpin>(writer: &mut W, value: u8) -> io::Result<()> {
    writer.write_all(&[value]).await
}

pub async fn read_u32<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<u32> {
    let mut buf = [0; 4];
    reader.read_exact(&mut buf).await?;
    Ok(u32::from_be_bytes(buf))
}

pub async fn read_u8<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<u8> {
    let mut buf = [0];
    reader.read_exact(&mut buf).await?;
    Ok(buf[0])
}
