use btrs::conn::{announce, handshake, Handshake};
use btrs::torrent::TorrentFile;
use tokio::fs::File;
use tokio::prelude::*;
use btrs::peer;

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
    let h = Handshake {
        peer_id: &peer_id,
        infohash: &t.info_hash,
        extensions: Default::default(),
    };

    for peer in &response.peers {
        if let Err(e) = handshake(peer, &h).await {
            println!("{:?}: {:?}", peer, e);
        }
    }
}
