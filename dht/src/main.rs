use dht::{id::NodeId, Server};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let addr = "192.168.43.212:17742".parse()?; //"router.utorrent.com:6881";
    let f = async {
        let mut server = Server::new(6881).await?;
        server.boostrap(&[addr]).await?;
        server.announce(&NodeId::of_byte(1)).await?;
        Ok(())
    };
    dht::future::timeout(f, 10).await?;
    Ok(())
}
