use crate::announce::{DhtTracker, Tracker};
use crate::avg::SlidingAvg;
use crate::client::{AsyncStream, Client};
use crate::future::timeout;
use crate::metainfo::InfoHash;
use crate::msg::Message;
use crate::peer::{self, Peer, PeerId};
use crate::work::{Piece, PieceWork, WorkQueue};
use anyhow::Context;
use ben::decode::Dict;
use ben::Parser;
use futures::channel::mpsc::Sender;
use futures::future::poll_fn;
use futures::stream::FuturesUnordered;
use futures::{SinkExt, Stream};
use sha1::Sha1;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::task::Poll;
use std::time::Instant;
use tokio::io::AsyncWriteExt;

pub const HASH_LEN: usize = 20;
const MAX_REQUESTS: u32 = 500;
const MIN_REQUESTS: u32 = 2;
const MAX_BLOCK_SIZE: u32 = 0x4000;

pub struct TorrentFile {
    pub tracker_urls: HashSet<String>,
    pub info_hash: InfoHash,
    pub piece_hashes: Vec<u8>,
    pub piece_len: usize,
    pub length: usize,
    pub name: String,
}

impl fmt::Debug for TorrentFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TorrentFile")
            .field("tracker_urls", &self.tracker_urls)
            .field("info_hash", &self.info_hash)
            .field(
                "piece_hashes",
                &format!("[..; {}]", self.piece_hashes.len()),
            )
            .field("piece_len", &self.piece_len)
            .field("length", &self.length)
            .field("name", &self.name)
            .finish()
    }
}

impl TorrentFile {
    pub fn parse(bytes: impl AsRef<[u8]>) -> anyhow::Result<TorrentFile> {
        let mut parser = Parser::new();
        let dict = parser.parse::<Dict>(bytes.as_ref())?;
        let announce = dict.get_str("announce").context("`announce` not found")?;
        let info = dict.get_dict("info").context("`info` dict not found")?;
        let info_bytes = info.as_raw_bytes();
        let info_hash = Sha1::from(info_bytes).digest().bytes().into();

        let length = info.get_int("length").context("`length` not found")?;
        let name = info.get_str("name").unwrap_or_default();
        let piece_len = info
            .get_int("piece length")
            .context("`piece length` not found")?;
        let pieces = info.get_bytes("pieces").context("`pieces` not found")?;

        let mut tracker_urls = hashset![announce.to_owned()];
        if let Some(list) = dict.get_list("announce-list") {
            for v in list.iter() {
                for v in v.as_list().context("`announce-list` is not a list")?.iter() {
                    tracker_urls.insert(
                        v.as_str()
                            .context("URL in `announce-list` is not a valid string")?
                            .to_string(),
                    );
                }
            }
        }

        let torrent = TorrentFile {
            tracker_urls,
            info_hash,
            piece_hashes: pieces.to_vec(),
            piece_len: piece_len as usize,
            length: length as usize,
            name: name.to_owned(),
        };

        Ok(torrent)
    }

    pub fn into_torrent(self) -> Torrent {
        let peer_id = peer::generate_peer_id();

        Torrent {
            peer_id,
            info_hash: self.info_hash,
            piece_hashes: self.piece_hashes,
            piece_len: self.piece_len,
            length: self.length,
            name: self.name,
            tracker_urls: self.tracker_urls,
            peers: hashset![],
            peers6: hashset![],
            dht_tracker: None,
        }
    }
}

pub struct Torrent {
    pub peer_id: PeerId,
    pub info_hash: InfoHash,
    pub piece_hashes: Vec<u8>,
    pub piece_len: usize,
    pub length: usize,
    pub name: String,
    pub tracker_urls: HashSet<String>,
    pub peers: HashSet<Peer>,
    pub peers6: HashSet<Peer>,
    pub dht_tracker: Option<DhtTracker>,
}

impl Torrent {
    pub fn worker(&mut self) -> TorrentWorker<'_> {
        TorrentWorker::new(self)
    }
}

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
                                    let mut dl = Download::new(client, work, piece_tx).await?;
                                    dl.download().await
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

struct PieceInProgress<'a> {
    piece: PieceWork<'a>,
    buf: Box<[u8]>,
    downloaded: u32,
    requested: u32,
}

struct Download<'w, 'p, C> {
    /// Peer connection
    client: Client<C>,

    /// Common piece queue from where we pick the pieces to download
    work: &'w WorkQueue<'p>,

    /// Channel to send the completed and verified pieces
    piece_tx: Sender<Piece>,

    /// In-progress pieces
    in_progress: HashMap<u32, PieceInProgress<'p>>,

    /// Current pending block requests
    backlog: u32,

    /// Max number of blocks that can be requested at once
    max_requests: u32,

    /// Piece block request count since last request
    last_requested_blocks: u32,

    /// Last time we requested pieces from this peer
    last_requested: Instant,

    /// Block download rate
    rate: SlidingAvg,
}

impl<C> Drop for Download<'_, '_, C> {
    fn drop(&mut self) {
        // Put any unfinished pieces back in the work queue
        self.work
            .borrow_mut()
            .extend(self.in_progress.drain().map(|(_i, p)| p.piece));
    }
}

impl<'w, 'p, C: AsyncStream> Download<'w, 'p, C> {
    async fn new(
        mut client: Client<C>,
        work: &'w WorkQueue<'p>,
        piece_tx: Sender<Piece>,
    ) -> anyhow::Result<Download<'w, 'p, C>> {
        client.send_unchoke().await?;
        client.send_interested().await?;
        client.conn.flush().await?;

        while client.choked {
            log::trace!("We're choked. Waiting for unchoke");
            if let Some(msg) = client.read().await? {
                log::warn!("Ignoring: {:?}", msg);
                msg.read_discard(&mut client.conn).await?;
            }
        }

        Ok(Download {
            client,
            work,
            piece_tx,
            in_progress: HashMap::new(),
            backlog: 0,
            max_requests: 5,
            last_requested_blocks: 0,
            last_requested: Instant::now(),
            rate: SlidingAvg::new(10),
        })
    }

    async fn download(&mut self) -> anyhow::Result<()> {
        log::trace!("download");
        loop {
            self.pick_pieces();

            log::trace!("Pending pieces: {}", self.in_progress.len());
            if self.in_progress.is_empty() && self.backlog == 0 {
                // No new pieces to download and no pending requests
                // We're done
                break;
            }

            self.fill_backlog().await?;

            log::trace!("Current backlog: {}", self.backlog);
            self.handle_msg().await?;
        }
        Ok(())
    }

    async fn handle_msg(&mut self) -> anyhow::Result<()> {
        let msg = timeout(self.client.read_in_loop(), 5).await?;

        let (index, len) = match msg {
            Message::Piece { index, len, .. } => (index, len),
            _ => {
                msg.read_discard(&mut self.client.conn).await?;
                return Ok(());
            }
        };

        let mut p = self
            .in_progress
            .remove(&index)
            .context("Received a piece that was not requested")?;

        msg.read_piece(&mut self.client.conn, &mut p.buf).await?;
        p.downloaded += len;
        self.backlog -= 1;
        log::trace!("current index {}: {}/{}", index, p.downloaded, p.piece.len);

        if p.downloaded < p.piece.len {
            // Not done yet
            self.in_progress.insert(index, p);
            return Ok(());
        }

        self.piece_done(p).await
    }

    async fn piece_done(&mut self, state: PieceInProgress<'p>) -> anyhow::Result<()> {
        log::trace!("Piece downloaded: {}", state.piece.index);
        if !state.piece.check_integrity(&state.buf) {
            log::error!("Bad piece: Hash mismatch for {}", state.piece.index);
            self.work.borrow_mut().push_back(state.piece);
            return Ok(());
        }

        log::info!("Downloaded and Verified {} piece", state.piece.index);
        self.client.send_have(state.piece.index).await?;
        let piece = Piece {
            index: state.piece.index,
            buf: state.buf,
        };
        self.piece_tx.send(piece).await?;
        Ok(())
    }

    fn pick_pieces(&mut self) {
        if self.backlog >= self.max_requests {
            // We need to wait for the backlog to come down to pick
            // new pieces
            return;
        }

        if let Some(piece) = self.work.borrow_mut().pop_front() {
            // Safety: This buffer is sent to the writer task for reading only
            // after being completely written by this download
            let buf = unsafe {
                let mut buf = Vec::with_capacity(piece.len as usize);
                buf.set_len(piece.len as usize);
                buf.into_boxed_slice()
            };
            self.in_progress.insert(
                piece.index,
                PieceInProgress {
                    piece,
                    buf,
                    downloaded: 0,
                    requested: 0,
                },
            );
        }
    }

    async fn fill_backlog(&mut self) -> anyhow::Result<()> {
        if self.client.choked || self.backlog >= MIN_REQUESTS {
            // Either
            // - Choked - Wait for peer to send us an Unchoke
            // - Too many pending requests - Wait for peer to send us already requested pieces.
            return Ok(());
        }

        self.adjust_watermark();

        let mut need_flush = false;

        for s in self.in_progress.values_mut() {
            while self.backlog < self.max_requests && s.requested < s.piece.len {
                let block_size = MAX_BLOCK_SIZE.min(s.piece.len - s.requested);
                let request = self
                    .client
                    .send_request(s.piece.index, s.requested, block_size);
                timeout(request, 5).await?;

                self.backlog += 1;
                s.requested += block_size;
                need_flush = true;
            }
        }

        if need_flush {
            self.last_requested_blocks = self.backlog;
            self.last_requested = Instant::now();

            log::trace!("Flushing the client");
            timeout(self.client.conn.flush(), 5).await
        } else {
            Ok(())
        }
    }

    fn adjust_watermark(&mut self) {
        log::debug!("Old max_requests: {}", self.max_requests);

        let millis = (Instant::now() - self.last_requested).as_millis();
        if millis == 0 {
            // Too high speed!
            return;
        }

        let blocks_done = self.last_requested_blocks - self.backlog;
        let blocks_per_sec = (1000 * blocks_done as u128 / millis) as i32;

        // Update the average block download rate
        self.rate.add_sample(blocks_per_sec);

        let rate = self.rate.mean() as u32;
        if rate > MIN_REQUESTS {
            self.max_requests = rate.min(MAX_REQUESTS);
        }

        log::debug!("New max_requests: {}", self.max_requests);
    }
}

pub struct PieceIter<'a> {
    piece_hashes: &'a [u8],
    piece_len: usize,
    length: usize,
    index: u32,
    count: u32,
}

impl<'a> PieceIter<'a> {
    fn new(piece_hashes: &'a [u8], piece_len: usize, length: usize) -> Self {
        Self {
            piece_hashes,
            piece_len,
            length,
            index: 0,
            count: (piece_hashes.len() / 20) as u32,
        }
    }
}

impl<'a> Iterator for PieceIter<'a> {
    type Item = PieceWork<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.count {
            return None;
        }

        let hash = &self.piece_hashes[self.index as usize * HASH_LEN..][..HASH_LEN];

        let piece_len = self.piece_len as u32;
        let start = self.index * piece_len;
        let len = piece_len.min(self.length as u32 - start);

        let piece = PieceWork {
            index: self.index,
            len,
            hash,
        };

        self.index += 1;

        Some(piece)
    }
}
