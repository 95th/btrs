use btrs::conn::{announce, handshake, Handshake};
use btrs::peer;
use btrs::torrent::TorrentFile;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use tokio::fs::File;
use tokio::prelude::*;

fn main() {
    let mut rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(open());
}

async fn open() {
    let mut f = File::open("t.torrent").await.unwrap();
    let mut v = Vec::new();
    f.read_to_end(&mut v).await.unwrap();
    let t = TorrentFile::parse(&v).unwrap();
    let peer_id = peer::generate_peer_id();
    let response = announce(&t, &peer_id, 6881).await.unwrap();
    println!("{:#?}", response);
    let h = &Handshake {
        peer_id: &peer_id,
        infohash: &t.info_hash,
        extensions: Default::default(),
    };

    let mut futs = FuturesUnordered::new();

    for peer in response.peers.iter() {
        futs.push(async move {
            if let Err(e) = handshake(peer, h, 3).await {
                println!("{:?}: {:?}", peer, e);
            }
        });
    }

    while let Some(_) = futs.next().await {
        println!("done");
    }
}
