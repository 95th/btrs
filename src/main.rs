use btrs::torrent::TorrentFile;
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
    btrs::conn::connect(&t).await.unwrap();
}
