use crate::announce::{announce, AnnounceResponse};
use crate::bitfield::BitField;
use crate::client::{AsyncStream, Client, Connection};
use crate::future::timeout;
use crate::metainfo::InfoHash;
use crate::msg::Message;
use crate::peer::{self, Peer, PeerId};
use crate::work::{Piece, PieceWork, WorkQueue};
use ben::Node;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use log::{debug, error, info, trace};
use sha1::Sha1;
use std::collections::VecDeque;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::Sender;

pub const HASH_LEN: usize = 20;
const BACKLOG_HI_WATERMARK: u32 = 100;
const BACKLOG_LO_WATERMARK: u32 = 10;
const MAX_BLOCK_SIZE: u32 = 16_384;

#[derive(Debug)]
pub struct TorrentFile {
    pub announce: String,
    pub info_hash: InfoHash,
    pub piece_hashes: Vec<u8>,
    pub piece_len: usize,
    pub length: usize,
    pub name: String,
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

        let torrent = TorrentFile {
            announce: announce.to_owned(),
            info_hash,
            piece_hashes: pieces.to_vec(),
            piece_len: piece_len as usize,
            length: length as usize,
            name: name.to_owned(),
        };

        Some(torrent)
    }

    pub async fn into_torrent(self) -> crate::Result<Torrent> {
        let peer_id = peer::generate_peer_id();
        debug!("Our peer_id: {:x?}", peer_id);

        debug!("Infohash: {:x?}", self.info_hash);
        let resp = announce(&self.announce, &self.info_hash, &peer_id, 6881).await?;
        let AnnounceResponse { peers, peers6, .. } = resp;

        Ok(Torrent {
            peers,
            peers6,
            peer_id,
            info_hash: self.info_hash,
            piece_hashes: self.piece_hashes,
            piece_len: self.piece_len,
            length: self.length,
            name: self.name,
        })
    }
}

pub struct Torrent {
    pub peers: Vec<Peer>,
    pub peers6: Vec<Peer>,
    pub peer_id: Box<PeerId>,
    pub info_hash: InfoHash,
    pub piece_hashes: Vec<u8>,
    pub piece_len: usize,
    pub length: usize,
    pub name: String,
}

impl Torrent {
    pub fn piece_iter(&self) -> PieceIter<'_> {
        PieceIter::new(self)
    }

    pub fn worker<C>(&self) -> TorrentWorker<'_, C> {
        let work: VecDeque<_> = self.piece_iter().collect();
        TorrentWorker {
            torrent: self,
            bits: BitField::new(work.len()),
            work: WorkQueue::new(work),
            connected: vec![],
        }
    }
}

pub struct TorrentWorker<'a, C> {
    torrent: &'a Torrent,
    pub bits: BitField,
    pub work: WorkQueue<'a>,
    pub connected: Vec<Client<C>>,
}

impl<'a> TorrentWorker<'a, Connection> {
    pub async fn connect_all(&mut self) -> usize {
        let info_hash = &self.torrent.info_hash;
        let peer_id = &self.torrent.peer_id;
        let mut clients: FuturesUnordered<_> = self
            .torrent
            .peers
            .iter()
            .chain(self.torrent.peers6.iter())
            .map(|peer| async move {
                let mut client = timeout(Client::new_tcp(peer.addr), 3).await?;
                client.handshake(info_hash, peer_id).await?;
                Ok::<_, crate::Error>(client)
            })
            .collect();

        while let Some(result) = clients.next().await {
            match result {
                Ok(client) => self.connected.push(client),
                Err(e) => debug!("Error occurred: {}", e),
            }
        }

        debug!("{} peers connected", self.connected.len());
        self.connected.len()
    }
}

impl<'a, C: AsyncStream> TorrentWorker<'a, C> {
    pub async fn run_worker(&mut self, piece_tx: Sender<Piece>) {
        let work = &self.work;
        let mut futures = self
            .connected
            .iter_mut()
            .map(|client| {
                let piece_tx = piece_tx.clone();
                async move {
                    let dl = Download::new(client, work, piece_tx).await?;
                    dl.download().await
                }
            })
            .collect::<FuturesUnordered<_>>();

        while let Some(result) = futures.next().await {
            if let Err(e) = result {
                debug!("Error occurred: {}", e);
            }
        }
    }
}

struct PieceProgress<'a> {
    piece: PieceWork<'a>,
    buf: Vec<u8>,
    downloaded: u32,
    requested: u32,
}

struct Download<'a, 'p, C> {
    client: &'a mut Client<C>,
    work: &'a WorkQueue<'p>,
    piece_tx: Sender<Piece>,
    queue: VecDeque<PieceProgress<'p>>,
    backlog: u32,
}

impl<'a, 'p, C: AsyncStream> Download<'a, 'p, C> {
    async fn new(
        client: &'a mut Client<C>,
        work: &'a WorkQueue<'p>,
        piece_tx: Sender<Piece>,
    ) -> crate::Result<Download<'a, 'p, C>> {
        client.send_unchoke().await?;
        client.send_interested().await?;
        client.conn.flush().await?;

        while client.choked {
            trace!("We're choked. Waiting for unchoke");
            if let Some(msg) = client.read().await? {
                debug!("Ignoring: {:?}", msg);
                msg.read_discard(&mut client.conn).await?;
            }
        }

        let queue = VecDeque::new();
        Ok(Download {
            client,
            work,
            piece_tx,
            queue,
            backlog: 0,
        })
    }

    async fn download(mut self) -> crate::Result<()> {
        if let Err(e) = self.attempt_download().await {
            // In case of failure, put the pending pieces back into the queue
            self.work
                .borrow_mut()
                .extend(self.queue.into_iter().map(|p| p.piece));
            return Err(e);
        };
        Ok(())
    }

    async fn attempt_download(&mut self) -> crate::Result<()> {
        trace!("attempt_download");
        loop {
            self.pick_pieces();

            trace!("Pending pieces: {}", self.queue.len());
            if self.queue.is_empty() && self.backlog == 0 {
                break;
            }

            self.fill_backlog().await?;

            trace!("Current backlog: {}", self.backlog);
            self.handle_msg().await?;
        }
        Ok(())
    }

    async fn handle_msg(&mut self) -> crate::Result<()> {
        let msg = timeout(self.client.read_in_loop(), 30).await?;

        let (index, len) = match msg {
            Message::Piece { index, len, .. } => (index, len),
            _ => {
                msg.read_discard(&mut self.client.conn).await?;
                return Ok(());
            }
        };

        let i = match self.queue.iter().position(|s| s.piece.index == index) {
            Some(i) => i,
            _ => return Err("Received a piece that was not requested".into()),
        };

        let mut piece_progress = self.queue.swap_remove_back(i).unwrap();
        msg.read_piece(&mut self.client.conn, &mut piece_progress.buf).await?;
        piece_progress.downloaded += len;
        self.backlog -= 1;
        trace!(
            "current index {}: {}/{}",
            index,
            piece_progress.downloaded,
            piece_progress.piece.len
        );

        if piece_progress.downloaded < piece_progress.piece.len {
            // Not done yet
            self.queue.push_back(piece_progress);
            return Ok(());
        }

        self.piece_done(piece_progress).await
    }

    async fn piece_done(&mut self, state: PieceProgress<'p>) -> crate::Result<()> {
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
        if self.backlog >= BACKLOG_HI_WATERMARK {
            return;
        }

        if let Some(piece) = self.work.borrow_mut().pop_front() {
            let buf = vec![0; piece.len as usize];
            self.queue.push_back(PieceProgress {
                piece,
                buf,
                downloaded: 0,
                requested: 0,
            });
        }
    }

    async fn fill_backlog(&mut self) -> crate::Result<()> {
        if self.backlog >= BACKLOG_LO_WATERMARK || self.client.choked {
            return Ok(());
        }

        for s in self.queue.iter_mut() {
            while self.backlog < BACKLOG_HI_WATERMARK && s.requested < s.piece.len {
                let block_size = MAX_BLOCK_SIZE.min(s.piece.len - s.requested);
                let request = self
                    .client
                    .send_request(s.piece.index, s.requested, block_size);
                timeout(request, 5).await?;

                self.backlog += 1;
                s.requested += block_size;
            }
        }
        trace!("Flushing the client");
        timeout(self.client.conn.flush(), 5).await
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
