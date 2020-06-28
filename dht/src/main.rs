use dht::{id::NodeId, Server};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let addrs = &[
        "192.168.43.212:17742".parse()?,
        "82.221.103.244:6881".parse()?,
    ];
    let f = async {
        let mut server = Server::new(6881).await?;
        server.boostrap(addrs).await?;
        let info_hash = NodeId::from_hex(b"e8f5dec8c3e35f090a105da0da865d77099cf59e").unwrap();
        server.get_peers(&info_hash).await
    };
    dht::future::timeout(f, 10).await?;
    Ok(())
}
