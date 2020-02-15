use btrs::bitfield::BitField;
use btrs::magnet::MagnetUri;
use btrs::peer;
use btrs::torrent::TorrentFile;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use log::debug;
use std::collections::VecDeque;
use tokio::fs;
use tokio::sync::{mpsc, Mutex};

#[tokio::main]
async fn main() -> btrs::Result<()> {
    env_logger::init();
    torrent_file().await
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

    // torrent.worker().connect_all().await;

    let work_queue: VecDeque<_> = torrent.piece_iter().collect();
    let num_pieces = work_queue.len();
    let work_queue = Mutex::new(work_queue);
    let (result_tx, mut result_rx) = mpsc::channel(200);

    let mut tasks = torrent
        .peers
        .iter()
        .chain(&torrent.peers6)
        .map(|peer| torrent.start_worker(&peer, &work_queue, result_tx.clone()))
        .collect::<FuturesUnordered<_>>();

    drop(result_tx);

    let len = torrent.length;
    let piece_len = torrent.piece_len;

    let handle = tokio::spawn(async move {
        let mut file = vec![0; len];
        let mut bitfield = BitField::new(num_pieces);

        while let Some(piece) = result_rx.next().await {
            if bitfield.get(piece.index) {
                panic!("Duplicate piece downloaded: {}", piece.index);
            }
            let start = piece.index * piece_len;
            let end = len.min(start + piece_len);
            file[start..end].copy_from_slice(&piece.buf);
            bitfield.set(piece.index, true);
        }
    });

    while let Some(result) = tasks.next().await {
        if let Err(e) = result {
            debug!("{}", e);
        }
    }

    handle.await.unwrap();
    Ok(())
}
