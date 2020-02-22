use btrs::bitfield::BitField;
use btrs::magnet::MagnetUri;
use btrs::peer;
use btrs::torrent::TorrentFile;
use btrs::work::Piece;
use futures::StreamExt;
use log::debug;
use tokio::fs;
use tokio::sync::mpsc;

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
