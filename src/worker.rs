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
use futures::{channel::mpsc::Sender, future::poll_fn, stream::FuturesUnordered, Stream};
use std::{
    collections::{HashSet, VecDeque},
    task::Poll,
};

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

        let piece_iter = PieceIter::new(&torrent.piece_hashes, torrent.piece_len, torrent.length);
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

    pub async fn run_worker(&mut self, piece_tx: Sender<Piece>) {
        let work = &self.work;
        let info_hash = &*self.info_hash;
        let peer_id = &*self.peer_id;
        let all_peers = &mut *self.peers;
        let all_peers6 = &mut *self.peers6;
        let trackers = &mut self.trackers;

        let mut new_dht;
        let mut dht = match &mut self.dht_tracker {
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
        let pending_dht_trackers = FuturesUnordered::new();

        futures::pin_mut!(pending_downloads);
        futures::pin_mut!(pending_trackers);
        futures::pin_mut!(pending_dht_trackers);

        // TODO: Make this configurable
        let max_connections = 10;
        let mut connected: Vec<Peer> = vec![];
        let mut failed: Vec<Peer> = vec![];

        let future = poll_fn(|cx| {
            loop {
                // No remaining pieces are left and no pending downloads
                if work.borrow().is_empty() && pending_downloads.is_empty() {
                    break;
                }

                if let Some(dht) = dht.take() {
                    pending_dht_trackers.push(async move {
                        let peers = dht.announce(info_hash).await;
                        (peers, dht)
                    });
                }

                // Announce
                while let Some(mut tracker) = trackers.pop_front() {
                    pending_trackers.push(async move {
                        let resp = tracker.announce(info_hash, peer_id).await;
                        (resp, tracker)
                    });
                }

                // Add new peer to download
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
                                    let mut download =
                                        Download::new(client, work, piece_tx).await?;
                                    download.start().await
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

                let mut tracker_pending = false;

                match pending_trackers.as_mut().poll_next(cx) {
                    Poll::Ready(Some((resp, tracker))) => {
                        match resp {
                            Ok(resp) => {
                                trackers.push_back(tracker);

                                all_peers.extend(resp.peers);
                                all_peers6.extend(resp.peers6);

                                // We don't want to connect failed peers again
                                all_peers.retain(|p| !failed.contains(p));
                                all_peers6.retain(|p| !failed.contains(p));
                            }
                            Err(e) => log::warn!("Announce error: {}", e),
                        }
                    }
                    Poll::Ready(None) => {}
                    Poll::Pending => tracker_pending = true,
                }

                match pending_dht_trackers.as_mut().poll_next(cx) {
                    Poll::Ready(Some((resp, tracker))) => {
                        match resp {
                            Ok(peers) => {
                                dht.replace(tracker);
                                all_peers.extend(peers);

                                // We don't want to connect failed peers again
                                all_peers.retain(|p| !failed.contains(p));
                                all_peers6.retain(|p| !failed.contains(p));
                            }
                            Err(e) => log::warn!("DHT announce error: {}", e),
                        }
                    }
                    Poll::Ready(None) => {}
                    Poll::Pending => tracker_pending = true,
                }

                match futures::ready!(pending_downloads.as_mut().poll_next(cx)) {
                    Some(result) => {
                        if let Err((e, peer)) = result {
                            log::warn!("Error occurred for peer {} : {}", peer.addr, e);
                            match connected.iter().position(|p| *p == peer) {
                                Some(pos) => {
                                    connected.swap_remove(pos);
                                    failed.push(peer);
                                }
                                None => debug_assert!(false, "peer should be in `connected` list"),
                            }
                        }
                    }
                    None => {
                        if tracker_pending {
                            return Poll::Pending;
                        } else if trackers.is_empty() && pending_trackers.is_empty() {
                            break;
                        }
                    }
                }
            }
            Poll::Ready(())
        });

        future.await
    }
}
