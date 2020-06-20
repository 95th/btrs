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
        server.get_peers(&NodeId::of_byte(1)).await
    };
    dht::future::timeout(f, 10).await?;
    Ok(())
}
