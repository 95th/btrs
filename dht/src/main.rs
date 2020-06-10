use dht::Server;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let addr = "192.168.43.212:17742".parse()?; //"router.utorrent.com:6881";
    let mut server = Server::new(6881).await?;
    dht::future::timeout(server.boostrap(&[addr]), 2).await?;
    Ok(())
}
