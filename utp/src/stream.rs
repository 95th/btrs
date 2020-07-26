use crate::socket::UtpSocket;
use std::io::Result;
use std::net::SocketAddr;
use tokio::net::ToSocketAddrs;

/// A structure that represents a uTP (Micro Transport Protocol) stream between a local socket and a
/// remote socket.
///
/// The connection will be closed when the value is dropped (either explicitly or when it goes out
/// of scope).
///
/// The default maximum retransmission retries is 5, which translates to about 16 seconds. It can be
/// changed by calling `set_max_retransmission_retries`. Notice that the initial congestion timeout
/// is 500 ms and doubles with each timeout.
///
/// # Examples
///
/// ```no_run
/// # async fn dox() {
/// use utp::UtpStream;
///
/// let mut stream = UtpStream::bind("127.0.0.1:1234").await.expect("Error binding stream");
/// let _ = stream.write(&[1]).await.unwrap();
/// let _ = stream.read(&mut [0; 1000]).await.unwrap();
/// # }
/// ```
pub struct UtpStream {
    socket: UtpSocket,
}

impl UtpStream {
    /// Creates a uTP stream listening on the given address.
    ///
    /// The address type can be any implementer of the `ToSocketAddr` trait. See its documentation
    /// for concrete examples.
    ///
    /// If more than one valid address is specified, only the first will be used.
    pub async fn bind<A: ToSocketAddrs>(addr: A) -> Result<UtpStream> {
        UtpSocket::bind(addr).await.map(|s| UtpStream { socket: s })
    }

    /// Opens a uTP connection to a remote host by hostname or IP address.
    ///
    /// The address type can be any implementer of the `ToSocketAddr` trait. See its documentation
    /// for concrete examples.
    ///
    /// If more than one valid address is specified, only the first will be used.
    pub async fn connect<A: ToSocketAddrs>(dst: A) -> Result<UtpStream> {
        // Port 0 means the operating system gets to choose it
        UtpSocket::connect(dst)
            .await
            .map(|s| UtpStream { socket: s })
    }

    /// Gracefully closes connection to peer.
    ///
    /// This method allows both peers to receive all packets still in
    /// flight.
    pub async fn close(&mut self) -> Result<()> {
        self.socket.close().await
    }

    /// Returns the socket address of the local half of this uTP connection.
    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.socket.local_addr()
    }

    /// Changes the maximum number of retransmission retries on the underlying socket.
    pub fn set_max_retransmission_retries(&mut self, n: u32) {
        self.socket.max_retransmission_retries = n;
    }

    /// Write given buffer over this stream
    pub async fn write(&mut self, buf: &[u8]) -> Result<usize> {
        self.socket.send_to(buf).await
    }

    /// Read into given buffer over this stream
    pub async fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let (n, _src) = self.socket.recv_from(buf).await?;
        Ok(n)
    }

    /// Read into given buffer over this stream
    pub async fn read_to_end(&mut self, buf: &mut Vec<u8>) -> Result<()> {
        let mut b = [0u8; 32];
        loop {
            let n = self.read(&mut b).await?;
            if n == 0 {
                break;
            }
            buf.extend(&b[..n]);
        }
        Ok(())
    }
}

impl From<UtpSocket> for UtpStream {
    fn from(socket: UtpSocket) -> Self {
        UtpStream { socket: socket }
    }
}

impl AsMut<UtpSocket> for UtpStream {
    fn as_mut(&mut self) -> &mut UtpSocket {
        &mut self.socket
    }
}
