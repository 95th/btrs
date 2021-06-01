use crate::{
    announce::{DhtTracker, Tracker},
    client::Client,
    download::Download,
    future::timeout,
    metainfo::InfoHash,
    peer::{Peer, PeerId},
    torrent::Torrent,
    work::{Piece, WorkQueue},
};
use futures::{
    channel::mpsc::{self, Sender},
    select,
    stream::{self, FuturesUnordered},
    FutureExt, SinkExt, StreamExt,
};
use std::{
    collections::{HashSet, VecDeque},
    time::Duration,
};
use tokio::time;

pub struct TorrentWorker<'a> {
    peer_id: &'a PeerId,
    info_hash: &'a InfoHash,
    work: WorkQueue,
    trackers: VecDeque<Tracker<'a>>,
    peers: &'a mut HashSet<Peer>,
    peers6: &'a mut HashSet<Peer>,
    dht_tracker: &'a mut DhtTracker,
}

impl<'a> TorrentWorker<'a> {
    pub fn new(torrent: &'a mut Torrent) -> Self {
        let trackers = torrent
            .tracker_urls
            .iter()
            .map(|url| Tracker::new(url))
            .collect();

        let work = WorkQueue::new(&torrent);

        Self {
            peer_id: &torrent.peer_id,
            info_hash: &torrent.info_hash,
            peers: &mut torrent.peers,
            peers6: &mut torrent.peers6,
            work,
            trackers,
            dht_tracker: &mut torrent.dht_tracker,
        }
    }

    pub fn num_pieces(&self) -> usize {
        self.work.len()
    }

    pub async fn run(&mut self, piece_tx: Sender<Piece>) {
        let work = &self.work;
        let info_hash = &*self.info_hash;
        let peer_id = &*self.peer_id;
        let all_peers = &mut *self.peers;
        let all_peers6 = &mut *self.peers6;
        let trackers = &mut self.trackers;
        let dht_tracker = &mut *self.dht_tracker;

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
                                let f = async {
                                    let mut client = timeout(Client::new_tcp(peer.addr), 3).await?;
                                    client.handshake(info_hash, peer_id).await?;
                                    let mut dl = Download::new(client, work, piece_tx).await?;
                                    dl.start().await
                                };
                                f.await.map_err(|e| (e, peer))
                            });

                            connected.insert(peer);

                            log::debug!(
                                "{} active connections, {} pending trackers, {} pending downloads",
                                connected.len(),
                                pending_trackers.len(),
                                pending_downloads.len()
                            );
                        }
                    }
                }

                // Check running downloads
                maybe_result = pending_downloads.next() => {
                    match maybe_result {
                        Some(Ok(())) => {},
                        Some(Err((e, peer))) => {
                            log::warn!("Error occurred for peer {} : {}", peer.addr, e);

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
                            log::warn!("DHT announce error: {}", e);
                            break;
                        },
                        None => {
                            log::debug!("DHT Tracker is done");
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
                            log::debug!("Trackers are all done");
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
                       Err(e) => log::warn!("Announce error: {}", e),
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
