use crate::{
    announce::{DhtTracker, Tracker},
    client::Client,
    download::Download,
    future::timeout,
    metainfo::InfoHash,
    peer::{Peer, PeerId},
    torrent::Torrent,
    work::{Piece, PieceIter, WorkQueue},
};
use futures::{
    channel::mpsc::{self, Sender},
    select,
    stream::{self, FuturesUnordered},
    SinkExt, StreamExt,
};
use std::collections::{HashSet, VecDeque};

const SHA_1: usize = 20;

pub struct TorrentWorker<'a> {
    peer_id: &'a PeerId,
    info_hash: &'a InfoHash,
    work: WorkQueue<'a>,
    trackers: VecDeque<Tracker<'a>>,
    peers: &'a mut HashSet<Peer>,
    peers6: &'a mut HashSet<Peer>,
    dht_tracker: Option<&'a mut DhtTracker>,
}

impl<'a> TorrentWorker<'a> {
    pub fn new(torrent: &'a mut Torrent) -> Self {
        let trackers = torrent
            .tracker_urls
            .iter()
            .map(|url| Tracker::new(url))
            .collect();

        let piece_iter =
            PieceIter::<SHA_1>::new(&torrent.piece_hashes, torrent.piece_len, torrent.length);
        let work = WorkQueue::new(piece_iter.collect());

        Self {
            peer_id: &torrent.peer_id,
            info_hash: &torrent.info_hash,
            peers: &mut torrent.peers,
            peers6: &mut torrent.peers6,
            work,
            trackers,
            dht_tracker: torrent.dht_tracker.as_mut(),
        }
    }

    pub fn num_pieces(&self) -> usize {
        self.work.borrow().len()
    }

    pub async fn run(&mut self, piece_tx: Sender<Piece>) {
        let work = &self.work;
        let info_hash = &*self.info_hash;
        let peer_id = &*self.peer_id;
        let all_peers = &mut *self.peers;
        let all_peers6 = &mut *self.peers6;
        let trackers = &mut self.trackers;

        let mut new_dht;
        let dht = match &mut self.dht_tracker {
            Some(dht) => Some(&mut **dht),
            None => match DhtTracker::new().await {
                Ok(d) => {
                    new_dht = Some(d);
                    new_dht.as_mut()
                }
                Err(e) => {
                    log::warn!("Dht failed to start: {}", e);
                    None
                }
            },
        };

        let pending_downloads = FuturesUnordered::new();
        let pending_trackers = FuturesUnordered::new();

        futures::pin_mut!(pending_downloads);
        futures::pin_mut!(pending_trackers);

        let dht_tracker = stream::unfold(dht, |dht| async {
            let dht = dht?;
            let peers = dht.announce(info_hash).await;
            Some((peers, Some(dht)))
        })
        .fuse();

        futures::pin_mut!(dht_tracker);

        // TODO: Make this configurable
        let max_connections = 10;
        let mut connected: Vec<Peer> = vec![];
        let mut failed: Vec<Peer> = vec![];

        let (mut add_conn_tx, mut add_conn_rx) = mpsc::channel(10);

        // Add initial connections
        if !all_peers.is_empty() || !all_peers6.is_empty() {
            add_conn_tx.send(()).await.unwrap();
        }

        loop {
            select! {
                // Add new download connections
                _ = add_conn_rx.next() => {
                    while connected.len() < max_connections {
                        let maybe_peer = all_peers
                            .iter()
                            .chain(all_peers6.iter())
                            .find(|p| !connected.contains(p) && !failed.contains(p));

                        if let Some(peer) = maybe_peer {
                            let dl = {
                                let peer = peer.clone();
                                let piece_tx = piece_tx.clone();
                                async move {
                                    let f = async {
                                        let mut client = timeout(Client::new_tcp(peer.addr), 3).await?;
                                        client.handshake(info_hash, peer_id).await?;
                                        let mut dl = Download::new(client, work, piece_tx).await?;
                                        dl.start().await
                                    };
                                    f.await.map_err(|e| (e, peer))
                                }
                            };
                            pending_downloads.push(dl);
                            connected.push(peer.clone());

                            log::trace!(
                                "{} active connections, {} pending trackers, {} pending downloads",
                                connected.len(),
                                pending_trackers.len(),
                                pending_downloads.len()
                            );
                        } else {
                            break;
                        }
                    }
                }

                // Check running downloads
                maybe_result = pending_downloads.next() => {
                    match maybe_result {
                        Some(Ok(())) => {},
                        Some(Err((e, peer))) => {
                            log::warn!("Error occurred for peer {} : {}", peer.addr, e);
                            match connected.iter().position(|p| *p == peer) {
                                Some(pos) => {
                                    connected.swap_remove(pos);
                                    failed.push(peer);
                                    add_conn_tx.send(()).await.unwrap();
                                }
                                None => debug_assert!(false, "peer should be in `connected` list"),
                            }
                        }
                        None => {
                            if work.borrow().is_empty() {
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
                        Some(Err(e)) => log::warn!("DHT announce error: {}", e),
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
            }
        }
    }
}
