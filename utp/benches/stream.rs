#![feature(test)]
extern crate test;

use std::sync::Arc;
use test::Bencher;
use tokio::runtime::Runtime;
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

#[bench]
fn bench_connection_setup_and_teardown(b: &mut Bencher) {
    let server_addr = next_test_ip4();
    let mut rt = Runtime::new().unwrap();
    b.iter(|| {
        let mut received = Vec::new();
        rt.block_on(async move {
            let mut server = UtpStream::bind(server_addr).await.unwrap();

            tokio::spawn(async move {
                let mut client = UtpStream::connect(server_addr).await.unwrap();
                client.close().await.unwrap();
            });

            server.read_to_end(&mut received).await.unwrap();
            server.close().await.unwrap();
        });
    });
}

#[bench]
fn bench_transfer_one_packet(b: &mut Bencher) {
    let len = 1024;
    let server_addr = next_test_ip4();
    let data = (0..len).map(|x| x as u8).collect::<Vec<u8>>();
    let data_arc = Arc::new(data);
    let mut rt = Runtime::new().unwrap();

    b.iter(|| {
        let mut received = Vec::with_capacity(len);
        let data = data_arc.clone();
        rt.block_on(async move {
            let mut server = UtpStream::bind(server_addr).await.unwrap();

            tokio::spawn(async move {
                let mut client = UtpStream::connect(server_addr).await.unwrap();
                client.write(&data[..]).await.unwrap();
                client.close().await.unwrap();
            });

            server.read_to_end(&mut received).await.unwrap();
            server.close().await.unwrap();
        });
    });
    b.bytes = len as u64;
}

#[bench]
fn bench_transfer_one_megabyte(b: &mut Bencher) {
    let len = 1024 * 1024;
    let server_addr = next_test_ip4();
    let data = (0..len).map(|x| x as u8).collect::<Vec<u8>>();
    let data_arc = Arc::new(data);
    let mut rt = Runtime::new().unwrap();

    b.iter(|| {
        let data = data_arc.clone();
        let mut received = Vec::with_capacity(len);
        rt.block_on(async move {
            let mut server = UtpStream::bind(server_addr).await.unwrap();

            tokio::spawn(async move {
                let mut client = UtpStream::connect(server_addr).await.unwrap();
                client.write(&data[..]).await.unwrap();
                client.close().await.unwrap();
            });

            server.read_to_end(&mut received).await.unwrap();
            server.close().await.unwrap();
        });
    });
    b.bytes = len as u64;
}
