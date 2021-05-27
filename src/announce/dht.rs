use crate::metainfo::InfoHash;
use crate::peer::Peer;
use dht::id::NodeId;
use dht::Dht;
use std::time::Duration;
use std::time::Instant;
use tokio::net::lookup_host;

pub struct DhtTracker {
    dht: Dht,
    next_announce: Instant,
}

impl DhtTracker {
    pub async fn new() -> anyhow::Result<Self> {
        let mut dht_routers = vec![];
        dht_routers.extend(lookup_host("dht.libtorrent.org:25401").await?);

        let (mut dht, server) = Dht::new(6881, dht_routers);
        tokio::spawn(server.run());

        dht.bootstrap().await?;

        Ok(Self {
            dht,
            next_announce: Instant::now(),
        })
    }

    pub async fn announce(&mut self, info_hash: &InfoHash) -> anyhow::Result<Vec<Peer>> {
        tokio::time::sleep_until(self.next_announce.into()).await;

        log::debug!("Announcing to DHT");
        let start = Instant::now();

        let peers = self.dht.announce(NodeId(*info_hash.as_ref())).await?;

        let took = Instant::now() - start;
        log::debug!(
            "Announce completed in {} ms, got {} peers",
            took.as_millis(),
            peers.len()
        );

        self.next_announce = Instant::now() + Duration::from_secs(15 * 60);
        Ok(peers.into_iter().map(|a| a.into()).collect())
    }
}
