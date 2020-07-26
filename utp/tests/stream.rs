use utp::UtpStream;

fn next_test_port() -> u16 {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT_OFFSET: AtomicUsize = AtomicUsize::new(0);
    const BASE_PORT: u16 = 9600;
    BASE_PORT + NEXT_OFFSET.fetch_add(1, Ordering::Relaxed) as u16
}

fn next_test_ip4<'a>() -> (&'a str, u16) {
    ("127.0.0.1", next_test_port())
}

fn next_test_ip6<'a>() -> (&'a str, u16) {
    ("::1", next_test_port())
}

#[tokio::test]
async fn test_stream_open_and_close() {
    let server_addr = next_test_ip4();
    let mut server = UtpStream::bind(server_addr).await.unwrap();

    let child = tokio::spawn(async move {
        let mut client = UtpStream::connect(server_addr).await.unwrap();
        client.close().await.unwrap();
    });

    let mut received = vec![];
    server.read_to_end(&mut received).await.unwrap();
    server.close().await.unwrap();
    assert!(child.await.is_ok());
}

#[tokio::test]
async fn test_stream_open_and_close_ipv6() {
    let server_addr = next_test_ip6();
    let mut server = UtpStream::bind(server_addr).await.unwrap();

    let child = tokio::spawn(async move {
        let mut client = UtpStream::connect(server_addr).await.unwrap();
        client.close().await.unwrap();
    });

    let mut received = vec![];
    server.read_to_end(&mut received).await.unwrap();
    server.close().await.unwrap();
    assert!(child.await.is_ok());
}

#[tokio::test]
async fn test_stream_small_data() {
    // Fits in a packet
    const LEN: usize = 1024;
    let data: Vec<u8> = (0..LEN).map(|idx| idx as u8).collect();
    assert_eq!(LEN, data.len());

    let d = data.clone();
    let server_addr = next_test_ip4();
    let mut server = UtpStream::bind(server_addr).await.unwrap();

    let child = tokio::spawn(async move {
        let mut client = UtpStream::connect(server_addr).await.unwrap();
        client.write(&d[..]).await.unwrap();
        client.close().await.unwrap();
    });

    let mut received = Vec::with_capacity(LEN);
    server.read_to_end(&mut received).await.unwrap();
    assert!(!received.is_empty());
    assert_eq!(received.len(), data.len());
    assert_eq!(received, data);
    assert!(child.await.is_ok());
}

#[tokio::test]
async fn test_stream_large_data() {
    // Has to be sent over several packets
    const LEN: usize = 1024 * 1024;
    let data: Vec<u8> = (0..LEN).map(|idx| idx as u8).collect();
    assert_eq!(LEN, data.len());

    let d = data.clone();
    let server_addr = next_test_ip4();
    let mut server = UtpStream::bind(server_addr).await.unwrap();

    let child = tokio::spawn(async move {
        let mut client = UtpStream::connect(server_addr).await.unwrap();
        client.write(&d[..]).await.unwrap();
        client.close().await.unwrap();
    });

    let mut received = Vec::with_capacity(LEN);
    server.read_to_end(&mut received).await.unwrap();
    assert!(!received.is_empty());
    assert_eq!(received.len(), data.len());
    assert_eq!(received, data);
    assert!(child.await.is_ok());
}

#[tokio::test]
async fn test_stream_successive_reads() {
    const LEN: usize = 1024;
    let data: Vec<u8> = (0..LEN).map(|idx| idx as u8).collect();
    assert_eq!(LEN, data.len());

    let d = data.clone();
    let server_addr = next_test_ip4();
    let mut server = UtpStream::bind(server_addr).await.unwrap();

    let child = tokio::spawn(async move {
        let mut client = UtpStream::connect(server_addr).await.unwrap();
        client.write(&d[..]).await.unwrap();
        client.close().await.unwrap();
    });

    let mut received = Vec::with_capacity(LEN);
    server.read_to_end(&mut received).await.unwrap();
    assert!(!received.is_empty());
    assert_eq!(received.len(), data.len());
    assert_eq!(received, data);

    assert_eq!(server.read(&mut received).await.unwrap(), 0);
    assert!(child.await.is_ok());
}

#[tokio::test]
async fn test_local_addr() {
    use std::net::ToSocketAddrs;

    let addr = next_test_ip4();
    let addr = addr.to_socket_addrs().unwrap().next().unwrap();
    let stream = UtpStream::bind(addr).await.unwrap();

    assert!(stream.local_addr().is_ok());
    assert_eq!(stream.local_addr().unwrap(), addr);
}
