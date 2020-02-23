use btrs::bitfield::BitField;
use btrs::magnet::MagnetUri;
use btrs::peer;
use btrs::torrent::TorrentFile;
use btrs::work::Piece;
use futures::StreamExt;
use log::debug;
use tokio::fs;
use tokio::sync::mpsc;

use clap::{App, Arg};

#[tokio::main]
async fn main() -> btrs::Result<()> {
    let m = App::new("BT rust")
        .version("0.1")
        .author("95th")
        .about("Bittorrent client in Rust")
        .arg(
            Arg::with_name("torrent/magnet")
                .help("The torrent file path or Magnet link")
                .required(true)
                .index(1),
        )
        .get_matches();

    let input = m.value_of("torrent/magnet").unwrap();
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

    magnet.request_metadata(peer_id).await?;
    todo!()
}

pub async fn torrent_file(file: &str) -> btrs::Result<()> {
    let buf = fs::read(file).await?;
    let torrent_file = TorrentFile::parse(buf).ok_or("Unable to parse torrent file")?;
    let torrent = torrent_file.into_torrent().await?;

    let mut worker = torrent.worker();
    let count = worker.connect_all().await;
    if count == 0 {
        return Err("No peer connected".into());
    }

    let num_pieces = worker.work.borrow().len();

    let (result_tx, mut result_rx) = mpsc::channel::<Piece>(200);

    let len = torrent.length;
    let piece_len = torrent.piece_len;

    let handle = tokio::spawn(async move {
        let mut file = vec![0; len];
        let mut bitfield = BitField::new(num_pieces);

        while let Some(piece) = result_rx.next().await {
            let idx = piece.index as usize;
            if bitfield.get(idx) {
                panic!("Duplicate piece downloaded: {}", piece.index);
            }
            let start = idx * piece_len;
            let end = len.min(start + piece_len);
            file[start..end].copy_from_slice(&piece.buf);
            bitfield.set(idx, true);
        }
        file
    });

    worker.run_worker(result_tx).await;
    let file = handle.await.unwrap();
    println!("File downloaded; size: {}", file.len());
    Ok(())
}
