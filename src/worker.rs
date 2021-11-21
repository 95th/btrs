use crate::{
    announce::{DhtTracker, Tracker},
    download::Download,
    future::timeout,
    work::{Piece, WorkQueue},
};
use client::{torrent::Torrent, Client, InfoHash, PeerId};
use futures::{
    channel::mpsc::{self, Sender},
    select,
    stream::{self, FuturesUnordered},
    FutureExt, SinkExt, StreamExt,
};
use std::{
    collections::{HashSet, VecDeque},
    net::SocketAddr,
    time::Duration,
};
use tokio::{net::TcpStream, time};
use tracing::Instrument;

pub struct TorrentWorker {
    peer_id: PeerId,
    info_hash: InfoHash,
    work: WorkQueue,
    trackers: Vec<String>,
    peers: HashSet<SocketAddr>,
    peers6: HashSet<SocketAddr>,
    dht_tracker: DhtTracker,
}

impl TorrentWorker {
    pub fn new(torrent: Torrent, peer_id: PeerId, dht: DhtTracker) -> Self {
        let work = WorkQueue::new(torrent.piece_len, torrent.length, torrent.piece_hashes);

        Self {
            peer_id,
            info_hash: torrent.info_hash,
            peers: torrent.peers,
            peers6: torrent.peers_v6,
            work,
            trackers: torrent.tracker_urls,
            dht_tracker: dht,
        }
    }

    pub fn num_pieces(&self) -> usize {
        self.work.len()
    }

    pub async fn run(&mut self, piece_tx: Sender<Piece>) {
        let work = &self.work;
        let info_hash = &self.info_hash;
        let peer_id = &self.peer_id;
        let mut all_peers = self.peers.iter().copied().collect::<HashSet<_>>();
        let mut all_peers6 = self.peers6.iter().copied().collect::<HashSet<_>>();
        let mut trackers = self
            .trackers
            .iter()
            .map(|t| Tracker::new(t.clone()))
            .collect::<VecDeque<_>>();
        let dht_tracker = &mut self.dht_tracker;

        let pending_downloads = FuturesUnordered::new();
        let pending_trackers = FuturesUnordered::new();

        futures::pin_mut!(pending_downloads);
        futures::pin_mut!(pending_trackers);

        let dht_tracker = stream::unfold(dht_tracker, |dht| async {
            let peers = dht.announce(info_hash).await;
            Some((peers, dht))
        })
        .fuse();

        futures::pin_mut!(dht_tracker);

        // TODO: Make this configurable
        let max_connections = 10;
        let mut connected = HashSet::new();
        let mut failed = HashSet::new();
        let mut to_connect = Vec::with_capacity(10);

        let (mut add_conn_tx, mut add_conn_rx) = mpsc::channel(10);

        // Add initial connections
        if !all_peers.is_empty() || !all_peers6.is_empty() {
            add_conn_tx.send(()).await.unwrap();
        }

        let mut print_speed_interval = time::interval(Duration::from_secs(1));

        loop {
            select! {
                // Add new download connections
                _ = add_conn_rx.next() => {
                    if connected.len() < max_connections {
                        to_connect.extend(
                            all_peers
                                .iter()
                                .chain(all_peers6.iter())
                                .filter(|&p| !connected.contains(p) && !failed.contains(p))
                                .take(max_connections - connected.len())
                                .copied(),
                        );

                        for peer in to_connect.drain(..) {
                            let piece_tx = piece_tx.clone();
                            pending_downloads.push(async move {
                                let span = info_span!("conn", addr = ?peer);
                                let f = async {
                                    let socket = timeout(TcpStream::connect(peer), 3).await?;
                                    let mut client = Client::new(socket);
                                    client.send_handshake(info_hash, peer_id).await?;
                                    client.recv_handshake(info_hash).await?;
                                    let mut dl = Download::new(client, work, piece_tx).await?;
                                    dl.start().await
                                };
                                f.instrument(span).await.map_err(|e| (e, peer))
                            });

                            connected.insert(peer);

                            debug!(
                                "{} active connections, {} pending trackers, {} pending downloads",
                                connected.len(),
                                pending_trackers.len(),
                                pending_downloads.len()
                            );
                        }
                    }
                }

                // Check pending downloads
                maybe_result = pending_downloads.next() => {
                    match maybe_result {
                        Some(Ok(())) => {},
                        Some(Err((e, peer))) => {
                            warn!("Error occurred for peer {} : {}", peer, e);

                            if connected.remove(&peer) {
                                failed.insert(peer);
                                add_conn_tx.send(()).await.unwrap();
                            } else {
                                debug_assert!(false, "peer should be in `connected` list")
                            }
                        }
                        None => {
                            if work.is_empty() {
                                break;
                            }
                        },
                    }
                }

                // Check DHT Tracker announce
                peers = dht_tracker.next() => {
                    match peers {
                        Some(Ok(peers)) => {
                            all_peers.extend(peers);

                            // We don't want to connect failed peers again
                            all_peers.retain(|p| !failed.contains(p));
                            all_peers6.retain(|p| !failed.contains(p));
                            add_conn_tx.send(()).await.unwrap();
                        }
                        Some(Err(e)) => {
                            warn!("DHT announce error: {}", e);
                            break;
                        },
                        None => {
                            debug!("DHT Tracker is done");
                        }
                    }
                }

                // Check other tracker announce
                resp = pending_trackers.next() => {
                    let resp = match resp {
                        Some((resp, tracker)) => {
                            trackers.push_back(tracker);
                            resp
                        },
                        None => {
                            debug!("Trackers are all done");
                            continue;
                        }
                    };

                    while let Some(mut tracker) = trackers.pop_front() {
                        pending_trackers.push(async move {
                            let resp = tracker.announce(info_hash, peer_id).await;
                            (resp, tracker)
                        });
                    }

                    match resp {
                        Ok(resp) => {
                            all_peers.extend(resp.peers);
                            all_peers6.extend(resp.peers6);

                            // We don't want to connect failed peers again
                            all_peers.retain(|p| !failed.contains(p));
                            all_peers6.retain(|p| !failed.contains(p));
                            add_conn_tx.send(()).await.unwrap();
                        }
                       Err(e) => warn!("Announce error: {}", e),
                    }
                }

                // Print download speed
                _ = print_speed_interval.tick().fuse() => {
                    let n = work.get_downloaded_and_reset();
                    println!("{} kBps", n / 1000);
                }
            }
        }
    }
}
