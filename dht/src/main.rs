use dht::Server;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let addrs = vec![
        "192.168.43.212:17742".parse()?,
        "82.221.103.244:6881".parse()?,
    ];
    let f = async {
        let server = Server::new(6881, addrs).await?;
        // let info_hash = NodeId::from_hex(b"e8f5dec8c3e35f090a105da0da865d77099cf59e").unwrap();
        server.run().await;
        Ok(())
    };
    dht::future::timeout(f, 60).await?;
    Ok(())
}
