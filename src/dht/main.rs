use dht::id::NodeId;
use dht::{ClientRequest, Server};
use std::time::Duration;
use tokio::sync::oneshot;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let addrs = vec![
        "192.168.43.212:17742".parse()?,
        "82.221.103.244:6881".parse()?,
    ];

    let server = Server::new(6881, addrs).await?;
    let client = server.new_client();
    tokio::spawn(server.run());

    let info_hash = NodeId::from_hex(b"d04480dfa670f72f591439b51a9f82dcc58711b5").unwrap();
    let (tx, rx) = oneshot::channel();
    client
        .tx
        .send(ClientRequest::GetPeers(info_hash, tx))
        .await
        .unwrap();

    match rx.await {
        Ok(peers) => println!("Found {} peers", peers.len()),
        Err(e) => println!("Error in receiving peers: {}", e),
    }

    tokio::time::sleep(Duration::from_secs(10000)).await;
    Ok(())
}
