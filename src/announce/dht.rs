use crate::metainfo::InfoHash;
use crate::peer::Peer;
use dht::id::NodeId;
use dht::{Client, ClientRequest, Server};
use std::time::Instant;
use tokio::sync::oneshot;

pub struct DhtTracker {
    client: Client,
    next_announce: Instant,
}

impl DhtTracker {
    pub async fn new() -> crate::Result<Self> {
        let addrs = vec![
            "192.168.43.212:17742".parse()?,
            "82.221.103.244:6881".parse()?,
        ];
        let server = Server::new(6881, addrs).await?;
        let client = server.new_client();
        tokio::spawn(server.run());
        Ok(Self {
            client,
            next_announce: Instant::now(),
        })
    }

    pub async fn announce(&mut self, info_hash: &InfoHash) -> crate::Result<Vec<Peer>> {
        tokio::time::delay_until(self.next_announce.into()).await;

        debug!("Announcing to DHT");
        let start = Instant::now();

        let (tx, rx) = oneshot::channel();
        self.client
            .tx
            .send(ClientRequest::Announce(NodeId(*info_hash.as_ref()), tx))
            .await?;

        let peers = rx.await?;
        let took = Instant::now() - start;
        debug!(
            "Announce completed in {} ms, got {} peers",
            took.as_millis(),
            peers.len()
        );

        Ok(peers
            .into_iter()
            .map(|a| Peer::new(a.ip(), a.port()))
            .collect())
    }

    pub async fn shutdown(&mut self) {
        self.client.tx.send(ClientRequest::Shutdown).await.ok();
    }
}
