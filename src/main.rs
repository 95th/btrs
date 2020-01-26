use btrs::conn::announce;
use btrs::peer;
use btrs::torrent::TorrentFile;
use futures::StreamExt;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::{mpsc, Mutex};

#[tokio::main]
async fn main() -> btrs::Result<()> {
    let buf = fs::read("t.torrent").await?;
    let torrent = TorrentFile::parse(buf).ok_or("Unable to parse torrent file")?;
    let peer_id = peer::generate_peer_id();
    let response = announce(&torrent, &peer_id, 6881).await?;

    println!("{:?}", response);

    let torrent = Arc::new(torrent);
    let work_queue = Arc::new(Mutex::new(torrent.piece_iter().collect()));
    let (result_tx, mut result_rx) = mpsc::channel(200);

    for peer in response.peers {
        let torrent = torrent.clone();
        let work_queue = work_queue.clone();
        let result_tx = result_tx.clone();

        tokio::spawn(async move {
            torrent.start_worker(peer, work_queue, result_tx).await;
        });
    }

    let mut file = vec![0; torrent.length];

    while let Some(piece) = result_rx.next().await {
        let bounds = torrent.piece_bounds(piece.index);
        file[bounds].copy_from_slice(&piece.buf);
    }

    Ok(())
}
