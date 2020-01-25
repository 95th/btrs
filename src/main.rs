use btrs::conn::{announce, Handshake};
use btrs::future::timeout;
use btrs::peer;
use btrs::torrent::TorrentFile;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use tokio::fs;

#[tokio::main]
async fn main() -> btrs::Result<()> {
    let buf = fs::read("t.torrent").await?;
    let torrent = TorrentFile::parse(buf).ok_or("Unable to parse torrent file")?;
    let peer_id = peer::generate_peer_id();
    let response = announce(&torrent, &peer_id, 6881).await?;

    println!("{:?}", response);

    let handshake = &Handshake::new(&torrent.info_hash, &peer_id);

    let mut futs: FuturesUnordered<_> = response
        .peers
        .iter()
        .map(|peer| {
            async move {
                if let Err(e) = timeout(handshake.send(peer), 10).await {
                    println!("{:?}: {:?}", peer, e);
                }
            }
        })
        .collect();

    while let Some(_) = futs.next().await {
        println!("done");
    }
    Ok(())
}
