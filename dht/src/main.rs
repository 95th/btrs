use dht::Server;

#[tokio::main]
async fn main() {
    env_logger::init();
    let addr = "192.168.43.212:17742"; //"router.utorrent.com:6881";
    let server = Server::boostrap(addr).await.unwrap();
}
