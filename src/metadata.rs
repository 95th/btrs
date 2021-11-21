use std::{collections::HashSet, net::SocketAddr};

use client::{InfoHash, PeerId};
use futures::{stream::FuturesUnordered, StreamExt};

use crate::announce::{DhtTracker, Tracker};

pub async fn get_peers(
    info_hash: &InfoHash,
    peer_id: &PeerId,
    trackers: &[String],
    dht_tracker: &mut DhtTracker,
) -> anyhow::Result<(HashSet<SocketAddr>, HashSet<SocketAddr>)> {
    debug!("Requesting peers");

    let mut futs: FuturesUnordered<_> = trackers
        .iter()
        .map(|url| async move {
            let mut t = Tracker::new(url.clone());
            t.announce(info_hash, peer_id).await
        })
        .collect();

    let mut peers = hashset![];
    let mut peers6 = hashset![];

    while let Some(r) = futs.next().await {
        match r {
            Ok(r) => {
                peers.extend(r.peers);
                peers6.extend(r.peers6);
            }
            Err(e) => debug!("Error: {}", e),
        }
    }

    debug!("Got {} v4 peers and {} v6 peers", peers.len(), peers6.len());

    if peers.is_empty() && peers6.is_empty() {
        if let Ok(p) = dht_tracker.announce(info_hash).await {
            peers.extend(p);
        }
        debug!(
            "Got {} v4 peers and {} v6 peers from DHT",
            peers.len(),
            peers6.len()
        );
    }

    if peers.is_empty() && peers6.is_empty() {
        anyhow::bail!("No peers received from trackers");
    }

    Ok((peers, peers6))
}
