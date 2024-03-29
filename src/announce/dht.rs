use client::InfoHash;
use dht::Dht;
use dht::NodeId;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use std::time::Duration;
use std::time::Instant;

pub struct DhtTracker {
    dht: Dht,
    next_announce: Instant,
}

impl DhtTracker {
    pub async fn new() -> anyhow::Result<Self> {
        let dht_routers = [
            "dht.libtorrent.org:25401",
            "router.utorrent.com:6881",
            "router.bittorrent.com:6881",
            "dht.transmissionbt.com:6881",
            "router.bitcomet.com:6881",
            "dht.aelitis.com:6881",
        ]
        .into_iter()
        .filter_map(|a| a.to_socket_addrs().ok())
        .flatten()
        .collect();

        let dht = Dht::new(6881, dht_routers).await?;

        Ok(Self {
            dht,
            next_announce: Instant::now(),
        })
    }

    pub async fn announce(&mut self, info_hash: &InfoHash) -> anyhow::Result<HashSet<SocketAddr>> {
        tokio::time::sleep_until(self.next_announce.into()).await;

        debug!("Announcing to DHT");
        let start = Instant::now();

        let peers = self.dht.announce(NodeId::from(*info_hash)).await?;

        let took = Instant::now() - start;
        debug!(
            "Announce completed in {} ms, got {} peers",
            took.as_millis(),
            peers.len()
        );

        self.next_announce = Instant::now() + Duration::from_secs(15 * 60);
        Ok(peers)
    }
}
