use crate::announce::{announce, AnnounceResponse};
use crate::bitfield::BitField;
use crate::client::Client;
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
const MAX_BACKLOG: u32 = 50;
const MIN_BACKLOG: u32 = 5;
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

    pub fn worker(&self) -> TorrentWorker<'_> {
        let work: VecDeque<_> = self.piece_iter().collect();
        TorrentWorker {
            torrent: self,
            bits: BitField::new(work.len()),
            work: WorkQueue::new(work),
            connected: vec![],
        }
    }
}

pub struct TorrentWorker<'a> {
    torrent: &'a Torrent,
    pub bits: BitField,
    pub work: WorkQueue<'a>,
    pub connected: Vec<Client>,
}

impl<'a> TorrentWorker<'a> {
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

    pub async fn run_worker(&mut self, result_tx: Sender<Piece>) {
        let work = &self.work;
        let mut futures = self
            .connected
            .iter_mut()
            .map(|client| download(client, work, result_tx.clone()))
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

async fn download(
    client: &mut Client,
    work: &WorkQueue<'_>,
    result_tx: Sender<Piece>,
) -> crate::Result<()> {
    client.send_unchoke().await?;
    client.send_interested().await?;
    client.flush().await?;

    while client.choked {
        trace!("We're choked. Waiting for unchoke");
        if let Some(msg) = client.read().await? {
            debug!("Ignoring: {:?}", msg);
            msg.read_discard(client).await?;
        }
    }

    let mut dl = VecDeque::new();
    if let Err(e) = attempt_download(client, &work, result_tx, &mut dl).await {
        // In case of failure, put the pending pieces back into the queue
        work.borrow_mut().extend(dl.into_iter().map(|p| p.piece));
        return Err(e);
    };

    Ok(())
}

async fn attempt_download<'a>(
    client: &mut Client,
    work: &WorkQueue<'a>,
    mut result_tx: Sender<Piece>,
    dl: &mut VecDeque<PieceProgress<'a>>,
) -> crate::Result<()> {
    trace!("attempt_download");
    let mut backlog = 0;
    loop {
        if backlog < MAX_BACKLOG {
            if let Some(piece) = work.borrow_mut().pop_front() {
                let buf = vec![0; piece.len as usize];
                dl.push_back(PieceProgress {
                    piece,
                    buf,
                    downloaded: 0,
                    requested: 0,
                });
            }
        }

        trace!("Pending pieces: {}", dl.len());
        if dl.is_empty() && backlog == 0 {
            break;
        }

        if backlog < MIN_BACKLOG && !client.choked {
            for s in dl.iter_mut() {
                while backlog < MAX_BACKLOG && s.requested < s.piece.len {
                    let block_size = MAX_BLOCK_SIZE.min(s.piece.len - s.requested);
                    let request = client.send_request(s.piece.index, s.requested, block_size);
                    request.await?;

                    backlog += 1;
                    s.requested += block_size;
                }
            }
            client.flush().await?;
        }

        trace!("Current backlog: {}", backlog);
        let msg = timeout(client.read_in_loop(), 30).await?;
        if let Message::Piece { index, len, .. } = msg {
            if let Some(i) = dl.iter().position(|s| s.piece.index == index) {
                let mut state = dl.swap_remove_back(i).unwrap();
                msg.read_piece(client, &mut state.buf).await?;
                state.downloaded += len;
                backlog -= 1;
                trace!(
                    "current state index {}: {}/{}",
                    index,
                    state.downloaded,
                    state.piece.len
                );
                if state.downloaded < state.piece.len {
                    // Not done yet
                    dl.push_back(state);
                } else {
                    trace!("Piece downloaded: {}", state.piece.index);
                    if !state.piece.check_integrity(&state.buf) {
                        error!("Bad piece: Hash mismatch for {}", state.piece.index);
                        work.borrow_mut().push_back(state.piece);
                        continue;
                    }

                    info!("Downloaded and Verified {} piece", state.piece.index);

                    client.send_have(state.piece.index).await?;
                    let piece = Piece {
                        index: state.piece.index,
                        buf: state.buf,
                    };
                    result_tx.send(piece).await?;
                }
            } else {
                return Err("Received a piece that was not requested".into());
            }
        }
    }
    Ok(())
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
