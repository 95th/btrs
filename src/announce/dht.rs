use crate::metainfo::InfoHash;
use crate::peer::Peer;
use dht::id::NodeId;
use dht::Dht;
use std::net::ToSocketAddrs;
use std::time::Duration;
use std::time::Instant;

pub struct DhtTracker {
    dht: Dht,
    next_announce: Instant,
    bootstapped: bool,
}

impl Default for DhtTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl DhtTracker {
    pub fn new() -> Self {
        let dht_routers = "dht.libtorrent.org:25401"
            .to_socket_addrs()
            .unwrap()
            .collect();
        let (dht, server) = Dht::new(6881, dht_routers);

        tokio::spawn(server.run());

        Self {
            dht,
            next_announce: Instant::now(),
            bootstapped: false,
        }
    }

    pub async fn announce(&mut self, info_hash: &InfoHash) -> anyhow::Result<Vec<Peer>> {
        tokio::time::sleep_until(self.next_announce.into()).await;

        log::debug!("Announcing to DHT");
        let start = Instant::now();

        if !self.bootstapped {
            self.dht.bootstrap().await?;
            self.bootstapped = true;
        }

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
