use btrs::dht::id::NodeId;
use btrs::dht::{ClientRequest, Server};
use std::{net::ToSocketAddrs, time::Duration};
use tokio::sync::oneshot;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let mut dht_routers = vec![];
    dht_routers.extend("dht.libtorrent.org:25401".to_socket_addrs()?);

    let server = Server::new(6881, dht_routers).await?;
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
