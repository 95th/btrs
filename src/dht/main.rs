use btrs::dht::id::NodeId;
use btrs::dht::{ClientRequest, Server};
use std::net::ToSocketAddrs;
use tokio::sync::mpsc;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let mut dht_routers = vec![];
    dht_routers.extend("dht.libtorrent.org:25401".to_socket_addrs()?);

    let server = Server::new(6881, dht_routers).await?;
    let client = server.new_client();
    tokio::spawn(server.run());

    let info_hash = NodeId::from_hex(b"d04480dfa670f72f591439b51a9f82dcc58711b5").unwrap();
    let (tx, mut rx) = mpsc::channel(100);
    client
        .tx
        .send(ClientRequest::GetPeers(info_hash, tx))
        .await
        .unwrap();

    while let Some(p) = rx.recv().await {
        println!("Got peer: {}", p);
    }

    Ok(())
}
