use btrs::bitfield::BitField;
use btrs::cache::Cache;
use btrs::magnet::MagnetUri;
use btrs::peer;
use btrs::torrent::{Torrent, TorrentFile};
use btrs::work::Piece;
use futures::StreamExt;
use log::{debug, error};
use std::fs;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use clap::{App, Arg};

#[tokio::main(basic_scheduler)]
async fn main() -> btrs::Result<()> {
    let m = App::new("BT rust")
        .version("0.1")
        .author("95th")
        .about("Bittorrent client in Rust")
        .arg(
            Arg::with_name("torrent|magnet")
                .help("The torrent file path or Magnet link")
                .required(true)
                .index(1),
        )
        .get_matches();

    let input = m.value_of("torrent|magnet").unwrap();
    env_logger::init();
    if input.starts_with("magnet") {
        magnet(input).await
    } else {
        torrent_file(input).await
    }
}

pub async fn magnet(uri: &str) -> btrs::Result<()> {
    let magnet = MagnetUri::parse_lenient(uri)?;
    let peer_id = peer::generate_peer_id();
    debug!("Our peer_id: {:?}", peer_id);

    let torrent = magnet.request_metadata(peer_id).await?;
    download(torrent).await
}

pub async fn torrent_file(file: &str) -> btrs::Result<()> {
    // let buf = fs::read(file)?;
    // let torrent_file = TorrentFile::parse(buf).ok_or("Unable to parse torrent file")?;
    // let torrent = torrent_file.into_torrent().await?;
    // download(torrent).await
    todo!()
}

pub async fn download(torrent: Torrent) -> btrs::Result<()> {
    let torrent_name = torrent.name.clone();
    let mut worker = torrent.worker();
    let num_pieces = worker.num_pieces();
    let piece_len = torrent.piece_len;

    let (piece_tx, mut piece_rx) = mpsc::channel::<Piece>(200);

    let handle = tokio::spawn(async move {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(torrent_name)
            .unwrap();
        let mut cache = Cache::new(&mut file, 50, piece_len);
        let mut bitfield = BitField::new(num_pieces);
        let mut downloaded = 0;
        let mut tick = Instant::now();

        while let Some(piece) = piece_rx.next().await {
            let idx = piece.index as usize;
            if bitfield.get(idx) {
                error!("Duplicate piece downloaded: {}", piece.index);
            }

            cache.push(piece).unwrap();
            bitfield.set(idx, true);

            downloaded += piece_len;
            let now = Instant::now();
            if now - tick >= Duration::from_secs(1) {
                println!(
                    "{} kBps",
                    downloaded / 1000 / (now - tick).as_secs() as usize
                );
                downloaded = 0;
                tick = now;
            }
        }
        cache.flush().unwrap();
        println!("All pieces downloaded: {}", bitfield.all_true());
        file
    });

    worker.run_worker(piece_tx).await;
    let file = handle.await.unwrap();
    println!("File downloaded; size: {}", file.metadata().unwrap().len());
    Ok(())
}
