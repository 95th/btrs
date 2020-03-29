use crate::announce::{Tracker, TrackerMgr};
use crate::avg::SlidingAvg;
use crate::client::{AsyncStream, Client};
use crate::future::timeout;
use crate::metainfo::InfoHash;
use crate::msg::Message;
use crate::peer::{self, Peer, PeerId};
use crate::work::{Piece, PieceWork, WorkQueue};
use ben::Node;
use futures::future::poll_fn;
use futures::stream::FuturesUnordered;
use futures::Stream;
use log::{debug, error, info, trace, warn};
use sha1::Sha1;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::task::Poll;
use std::time::Instant;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::Sender;

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
    pub fn parse(bytes: impl AsRef<[u8]>) -> Option<TorrentFile> {
        let value = Node::parse(bytes.as_ref()).ok()?;
        let dict = value.as_dict()?;
        let announce = dict.get_str(b"announce")?;
        let info_bytes = dict.get(b"info")?.data();
        let info_hash = Sha1::from(info_bytes).digest().bytes().into();

        let info_dict = dict.get_dict(b"info")?;
        let length = info_dict.get_int(b"length")?;
        let name = info_dict.get_str(b"name").unwrap_or_default();
        let piece_len = info_dict.get_int(b"piece length")?;
        let pieces = info_dict.get(b"pieces")?.data();

        let mut tracker_urls = hashset![announce.to_owned()];
        if let Some(list) = dict.get_list(b"announce-list") {
            for v in list.iter() {
                for v in v.as_list()?.iter() {
                    tracker_urls.insert(v.as_str()?.to_string());
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

        Some(torrent)
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
        }
    }
}

pub struct Torrent {
    pub peer_id: Box<PeerId>,
    pub info_hash: InfoHash,
    pub piece_hashes: Vec<u8>,
    pub piece_len: usize,
    pub length: usize,
    pub name: String,
    pub tracker_urls: HashSet<String>,
}

impl Torrent {
    pub fn piece_iter(&self) -> PieceIter<'_> {
        PieceIter::new(self)
    }

    pub fn worker<'a>(&'a self, mgr: TrackerMgr) -> TorrentWorker<'a> {
        let trackers = self
            .tracker_urls
            .iter()
            .cloned()
            .map(|url| Tracker::new(url, mgr.clone()))
            .collect();
        TorrentWorker {
            torrent: self,
            work: WorkQueue::new(self.piece_iter().collect()),
            peers: hashset![],
            peers6: hashset![],
            trackers,
        }
    }
}

pub struct TorrentWorker<'a> {
    torrent: &'a Torrent,
    work: WorkQueue<'a>,
    peers: HashSet<Peer>,
    peers6: HashSet<Peer>,
    trackers: VecDeque<Tracker>,
}

impl TorrentWorker<'_> {
    pub fn num_pieces(&self) -> usize {
        self.work.borrow().len()
    }

    pub async fn run_worker(&mut self, piece_tx: Sender<Piece>) {
        let work = &self.work;
        let info_hash = &self.torrent.info_hash;
        let peer_id = &self.torrent.peer_id;
        let all_peers = &mut self.peers;
        let all_peers6 = &mut self.peers6;
        let trackers = &mut self.trackers;

        let pending_downloads = FuturesUnordered::new();
        let pending_trackers = FuturesUnordered::new();

        futures::pin_mut!(pending_downloads);
        futures::pin_mut!(pending_trackers);

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

                // Announce
                while let Some(tracker) = trackers.pop_front() {
                    pending_trackers.push(tracker.announce(info_hash, peer_id));
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

                        trace!(
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
                        if let Some(resp) = resp {
                            all_peers.extend(resp.peers);
                            all_peers6.extend(resp.peers6);

                            // We don't want to connect failed peers again
                            all_peers.retain(|p| !failed.contains(p));
                            all_peers6.retain(|p| !failed.contains(p));
                        }
                        trackers.push_back(tracker);
                    }
                    Poll::Ready(None) => {}
                    Poll::Pending => tracker_pending = true,
                }

                match futures::ready!(pending_downloads.as_mut().poll_next(cx)) {
                    Some(result) => {
                        if let Err((e, peer)) = result {
                            warn!("Error occurred for peer {} : {}", peer.addr, e);
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
    buf: Vec<u8>,
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
            .extend(self.in_progress.drain().map(|(_idx, p)| p.piece));
    }
}

impl<'w, 'p, C: AsyncStream> Download<'w, 'p, C> {
    async fn new(
        mut client: Client<C>,
        work: &'w WorkQueue<'p>,
        piece_tx: Sender<Piece>,
    ) -> crate::Result<Download<'w, 'p, C>> {
        client.send_unchoke().await?;
        client.send_interested().await?;
        client.conn.flush().await?;

        while client.choked {
            trace!("We're choked. Waiting for unchoke");
            if let Some(msg) = client.read().await? {
                warn!("Ignoring: {:?}", msg);
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

    async fn download(&mut self) -> crate::Result<()> {
        trace!("download");
        loop {
            self.pick_pieces();

            trace!("Pending pieces: {}", self.in_progress.len());
            if self.in_progress.is_empty() && self.backlog == 0 {
                // No new pieces to download and no pending requests
                // We're done
                break;
            }

            self.fill_backlog().await?;

            trace!("Current backlog: {}", self.backlog);
            self.handle_msg().await?;
        }
        Ok(())
    }

    async fn handle_msg(&mut self) -> crate::Result<()> {
        let msg = timeout(self.client.read_in_loop(), 5).await?;

        let (index, len) = match msg {
            Message::Piece { index, len, .. } => (index, len),
            _ => {
                msg.read_discard(&mut self.client.conn).await?;
                return Ok(());
            }
        };

        let mut p = match self.in_progress.remove(&index) {
            Some(p) => p,
            _ => return Err("Received a piece that was not requested".into()),
        };

        msg.read_piece(&mut self.client.conn, &mut p.buf).await?;
        p.downloaded += len;
        self.backlog -= 1;
        trace!("current index {}: {}/{}", index, p.downloaded, p.piece.len);

        if p.downloaded < p.piece.len {
            // Not done yet
            self.in_progress.insert(index, p);
            return Ok(());
        }

        self.piece_done(p).await
    }

    async fn piece_done(&mut self, state: PieceInProgress<'p>) -> crate::Result<()> {
        trace!("Piece downloaded: {}", state.piece.index);
        if !state.piece.check_integrity(&state.buf) {
            error!("Bad piece: Hash mismatch for {}", state.piece.index);
            self.work.borrow_mut().push_back(state.piece);
            return Ok(());
        }

        info!("Downloaded and Verified {} piece", state.piece.index);
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
            let buf = vec![0; piece.len as usize];
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

    async fn fill_backlog(&mut self) -> crate::Result<()> {
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

            trace!("Flushing the client");
            timeout(self.client.conn.flush(), 5).await
        } else {
            Ok(())
        }
    }

    fn adjust_watermark(&mut self) {
        debug!("Old max_requests: {}", self.max_requests);

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

        debug!("New max_requests: {}", self.max_requests);
    }
}

pub struct PieceIter<'a> {
    torrent: &'a Torrent,
    index: u32,
    count: u32,
}

impl PieceIter<'_> {
    fn new(torrent: &Torrent) -> PieceIter {
        PieceIter {
            torrent,
            index: 0,
            count: (torrent.piece_hashes.len() / 20) as u32,
        }
    }
}

impl<'a> Iterator for PieceIter<'a> {
    type Item = PieceWork<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.count {
            return None;
        }

        let hash = &self.torrent.piece_hashes[self.index as usize * HASH_LEN..][..HASH_LEN];

        let piece_len = self.torrent.piece_len as u32;
        let start = self.index * piece_len;
        let end = start + piece_len;

        let piece = PieceWork {
            index: self.index,
            len: end.min(self.torrent.length as u32) - start,
            hash,
        };

        self.index += 1;

        Some(piece)
    }
}
