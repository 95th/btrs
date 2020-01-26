use btrs::torrent::TorrentFile;
use futures::StreamExt;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::{mpsc, Mutex};

#[tokio::main]
async fn main() -> btrs::Result<()> {
    env_logger::init();

    let buf = fs::read("t.torrent").await?;
    let torrent_file = TorrentFile::parse(buf).ok_or("Unable to parse torrent file")?;
    let torrent = torrent_file.to_torrent().await?;

    let torrent = Arc::new(torrent);
    let work_queue = Arc::new(Mutex::new(torrent.piece_iter().collect()));
    let (result_tx, mut result_rx) = mpsc::channel(200);

    for peer in &torrent.peers {
        let torrent = torrent.clone();
        let work_queue = work_queue.clone();
        let result_tx = result_tx.clone();
        let peer = peer.clone();

        tokio::spawn(async move {
            if let Err(e) = torrent.start_worker(peer, work_queue, result_tx).await {
                println!("Error occurred: {}", e);
            }
        });
    }

    let mut file = vec![0; torrent.length];

    while let Some(piece) = result_rx.next().await {
        let bounds = torrent.piece_bounds(piece.index);
        file[bounds].copy_from_slice(&piece.buf);
    }

    Ok(())
}
