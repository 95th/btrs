use btrs::magnet::MagnetUri;
use btrs::peer;
use btrs::torrent::TorrentFile;
use futures::StreamExt;
use log::debug;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::{mpsc, Mutex};

#[tokio::main(core_threads = 1)]
async fn main() -> btrs::Result<()> {
    env_logger::init();
    magnet().await
}

pub async fn magnet() -> btrs::Result<()> {
    let magnet = MagnetUri::parse_lenient("magnet:?xt=urn:btih:4GCTIH7RBHVFS6YKBYQAGGW4QJ26JREV&tr=http%3A%2F%2Fnyaa.tracker.wf%3A7777%2Fannounce&tr=http%3A%2F%2Fanidex.moe%3A6969%2Fannounce&tr=udp%3A%2F%2Ftracker.uw0.xyz%3A6969&tr=http%3A%2F%2Ftracker.anirena.com%3A80%2Fannounce&tr=udp%3A%2F%2Fopen.stealth.si%3A80%2Fannounce&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce&tr=udp%3A%2F%2Ftracker.coppersurfer.tk%3A6969%2Fannounce&tr=udp%3A%2F%2Fexodus.desync.com%3A6969%2Fannounce")?;
    let peer_id = peer::generate_peer_id();
    debug!("Our peer_id: {:?}", peer_id);

    magnet.request_metadata(peer_id).await?;
    todo!()
}

pub async fn torrent_file() -> btrs::Result<()> {
    let buf = fs::read("t.torrent").await?;
    let torrent_file = TorrentFile::parse(buf).ok_or("Unable to parse torrent file")?;
    let torrent = torrent_file.into_torrent().await?;

    let torrent = Arc::new(torrent);
    let work_queue = Arc::new(Mutex::new(torrent.piece_iter().collect()));
    let (result_tx, mut result_rx) = mpsc::channel(200);

    for peer in torrent.peers.iter().chain(torrent.peers6.iter()) {
        let torrent = torrent.clone();
        let work_queue = work_queue.clone();
        let result_tx = result_tx.clone();
        let peer = peer.clone();

        tokio::spawn(async move {
            if let Err(e) = torrent.start_worker(&peer, work_queue, result_tx).await {
                debug!("Error occurred: {}", e);
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
