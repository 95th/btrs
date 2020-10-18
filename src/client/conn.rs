use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, BufStream, ReadBuf};
use tokio::net::TcpStream;

pub trait AsyncStream: AsyncRead + AsyncWrite + Unpin {}

impl<T: AsyncRead + AsyncWrite + Unpin> AsyncStream for T {}

const DEF_CAPACITY: usize = 1024 * 1024; // 1 MiB

pub enum Connection {
    Tcp(BufStream<TcpStream>),
}

impl Connection {
    pub async fn new_tcp(addr: SocketAddr) -> io::Result<Self> {
        let tcp = TcpStream::connect(addr).await?;
        let stream = BufStream::with_capacity(DEF_CAPACITY, DEF_CAPACITY, tcp);
        Ok(Self::Tcp(stream))
    }
}

impl AsyncRead for Connection {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match &mut *self {
            Connection::Tcp(c) => Pin::new(c).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Connection {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        match &mut *self {
            Connection::Tcp(c) => Pin::new(c).poll_write(cx, buf),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        match &mut *self {
            Connection::Tcp(c) => Pin::new(c).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        match &mut *self {
            Connection::Tcp(c) => Pin::new(c).poll_shutdown(cx),
        }
    }
}
