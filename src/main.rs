use btrs::bitfield::BitField;
use btrs::magnet::MagnetUri;
use btrs::peer;
use btrs::storage::StorageWriter;
use btrs::torrent::{Torrent, TorrentFile};
use btrs::work::Piece;
use clap::{App, Arg};
use futures::channel::mpsc;
use futures::StreamExt;
use std::fs;
use std::time::{Duration, Instant};

#[tokio::main(flavor = "current_thread")]
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
    log::debug!("Our peer_id: {:?}", peer_id);

    let torrent = magnet.request_metadata(peer_id).await?;
    download(torrent).await
}

pub async fn torrent_file(file: &str) -> btrs::Result<()> {
    let buf = fs::read(file)?;
    let torrent_file = TorrentFile::parse(buf)?;

    log::trace!("Parsed torrent file: {:#?}", torrent_file);

    let torrent = torrent_file.into_torrent();
    download(torrent).await
}

pub async fn download(mut torrent: Torrent) -> btrs::Result<()> {
    let torrent_name = torrent.name.clone();
    let piece_len = torrent.piece_len;

    let mut worker = torrent.worker();
    let num_pieces = worker.num_pieces();

    let (piece_tx, piece_rx) = mpsc::channel::<Piece>(200);

    let writer_task = write_to_file(torrent_name, piece_len, num_pieces, piece_rx);
    let download_task = worker.run_worker(piece_tx);

    futures::join!(writer_task, download_task);
    Ok(())
}

async fn write_to_file(
    torrent_name: String,
    piece_len: usize,
    num_pieces: usize,
    mut piece_rx: mpsc::Receiver<Piece>,
) {
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(torrent_name)
        .unwrap();
    let mut storage = StorageWriter::new(&mut file, piece_len);
    let mut bitfield = BitField::new(num_pieces);
    let mut downloaded = 0;
    let mut tick = Instant::now();

    while let Some(piece) = piece_rx.next().await {
        let index = piece.index as usize;
        match bitfield.get(index) {
            Some(true) => log::error!("Duplicate piece downloaded: {}", index),
            None => log::error!("Unexpected piece downloaded: {}", index),
            _ => {}
        }

        storage.insert(piece).unwrap();
        bitfield.set(index, true);

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
    println!("All pieces downloaded: {}", bitfield.all_true());
    println!("File downloaded; size: {}", file.metadata().unwrap().len());
}
