use btrs::torrent::TorrentFile;
use tokio::fs::File;
use tokio::prelude::*;

fn main() {
    // let mut rt = tokio::runtime::Runtime::new().unwrap();
    // rt.block_on(open());
    println!("{}", 8_u8 >> 4);
}

pub async fn open() {
    let mut f = File::open("t.torrent").await.unwrap();
    let mut v = Vec::new();
    f.read_to_end(&mut v).await.unwrap();
    let t = TorrentFile::parse(&v).unwrap();
    let peer_id = "-AZ3020-012345678910";
    let response = btrs::conn::connect(&t, peer_id, 6881).await.unwrap();
    println!("{:#?}", response);
}
