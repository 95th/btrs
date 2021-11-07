use btrs::magnet::MagnetUri;
use btrs::peer;
use btrs::storage::StorageWriter;
use btrs::torrent::{Torrent, TorrentFile};
use btrs::work::Piece;
use clap::{App, Arg};
use client::proto::bitfield::Bitfield;
use futures::channel::mpsc;
use futures::StreamExt;
use std::fs;
use tracing::{debug, error, trace};
use tracing_subscriber::EnvFilter;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .compact()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

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

    if input.starts_with("magnet") {
        magnet(input).await
    } else {
        torrent_file(input).await
    }
}

pub async fn magnet(uri: &str) -> anyhow::Result<()> {
    let magnet = MagnetUri::parse_lenient(uri)?;
    let peer_id = peer::generate_peer_id();
    debug!("Our peer_id: {:?}", peer_id);

    let torrent = magnet.request_metadata(peer_id).await?;
    download(torrent).await
}

pub async fn torrent_file(file: &str) -> anyhow::Result<()> {
    let buf = fs::read(file)?;
    let torrent_file = TorrentFile::parse(buf)?;

    trace!("Parsed torrent file: {:#?}", torrent_file);

    let torrent = torrent_file.into_torrent().await?;
    download(torrent).await
}

pub async fn download(mut torrent: Torrent) -> anyhow::Result<()> {
    let torrent_name = torrent.name.clone();
    let piece_len = torrent.piece_len;

    let mut worker = torrent.worker();
    let num_pieces = worker.num_pieces();

    let (piece_tx, piece_rx) = mpsc::channel::<Piece>(200);

    let writer_task = write_to_file(torrent_name, piece_len, num_pieces, piece_rx);
    let download_task = worker.run(piece_tx);

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
    let mut bitfield = Bitfield::with_size(num_pieces);

    // Save a piece to storage {
    while let Some(piece) = piece_rx.next().await {
        let index = piece.index as usize;
        if bitfield.get_bit(index) {
            error!("Duplicate piece downloaded: {}", index);
        }

        storage.insert(piece).unwrap();
        bitfield.set_bit(index);
    }
    println!("All pieces downloaded: {}", bitfield.is_all_set());
    println!("File downloaded; size: {}", file.metadata().unwrap().len());
}
