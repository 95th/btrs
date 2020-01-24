use btrs::conn::{announce, Handshake};
use btrs::peer;
use btrs::torrent::TorrentFile;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use tokio::fs;

#[tokio::main]
async fn main() {
    open().await;
}

async fn open() {
    let v = fs::read("t.torrent").await.unwrap();
    let t = TorrentFile::parse(&v).unwrap();
    let peer_id = peer::generate_peer_id();
    let response = announce(&t, &peer_id, 6881).await.unwrap();

    println!("{:?}", response);

    let h = &Handshake::new(&t.info_hash, &peer_id);

    let mut futs = FuturesUnordered::new();

    for peer in response.peers.iter() {
        futs.push(async move {
            if let Err(e) = h.send(peer, 3).await {
                println!("{:?}: {:?}", peer, e);
            }
        });
    }

    while let Some(_) = futs.next().await {
        println!("done");
    }
}
