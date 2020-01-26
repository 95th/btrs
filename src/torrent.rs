use crate::client::Client;
use crate::conn::{announce, AnnounceResponse};
use crate::metainfo::InfoHash;
use crate::msg::MessageKind;
use crate::peer::{self, Peer, PeerId};
use crate::work::{PieceResult, PieceWork, WorkQueue};
use sha1::Sha1;
use std::convert::TryInto;
use std::ops::Range;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc::Sender;

pub const HASH_LEN: usize = 20;
const MAX_BACKLOG: usize = 5;
const MAX_BLOCK_SIZE: usize = 16_384;

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
        let value = bencode::ValueRef::decode(bytes.as_ref()).ok()?;
        let dict = value.as_dict()?;
        let announce = dict.get("announce")?.as_str()?;
        let info_dict = dict.get("info")?.as_dict()?;
        let info_bytes = dict.get("info")?.encode_to_vec();
        let info_hash = Sha1::from(info_bytes).digest().bytes().into();

        let length = info_dict.get("length")?.as_int()?;
        let name = info_dict
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or_default();
        let piece_len = info_dict.get("piece length")?.as_int()?;
        let pieces = info_dict.get("pieces")?.as_bytes()?;

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

    pub async fn to_torrent(self) -> crate::Result<Torrent> {
        let peer_id = peer::generate_peer_id();
        let AnnounceResponse { peers, peers6, .. } = announce(&self, &peer_id, 6881).await?;

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
    peer_id: PeerId,
    info_hash: InfoHash,
    piece_hashes: Vec<u8>,
    pub piece_len: usize,
    pub length: usize,
    pub name: String,
}

impl Torrent {
    pub fn piece_iter(&self) -> PieceIter {
        PieceIter::new(self)
    }

    pub fn piece_bounds(&self, idx: usize) -> Range<usize> {
        let start = idx * self.piece_len;
        let end = start + self.piece_len;
        start..end.min(self.length)
    }

    pub async fn start_worker(
        &self,
        peer: Peer,
        work_queue: WorkQueue,
        mut result_tx: Sender<PieceResult>,
    ) -> crate::Result<()> {
        let mut client = Client::new_tcp(peer, self.info_hash.clone(), self.peer_id).await?;
        client.send_unchoke().await?;
        client.send_interested().await?;

        loop {
            let wrk = match work_queue.lock().await.pop_front() {
                Some(v) => v,
                None => break,
            };

            if !client.bitfield.get(wrk.index) {
                work_queue.lock().await.push_back(wrk);
                continue;
            }

            let buf = match attempt_download(&mut client, &wrk).await {
                Ok(v) => v,
                Err(e) => {
                    work_queue.lock().await.push_back(wrk);
                    return Err(e);
                }
            };

            if !check_integrity(&buf, &wrk) {
                work_queue.lock().await.push_back(wrk);
                continue;
            }

            client.send_have(wrk.index).await?;
            result_tx
                .send(PieceResult {
                    index: wrk.index,
                    buf,
                })
                .await?;
        }
        Ok(())
    }
}

async fn attempt_download<C>(client: &mut Client<C>, pw: &PieceWork) -> crate::Result<Vec<u8>>
where
    C: AsyncRead + AsyncWrite + Unpin,
{
    let mut state = PieceProgress {
        index: pw.index,
        client,
        buf: vec![0; pw.len],
        downloaded: 0,
        requested: 0,
        backlog: 0,
    };
    while state.downloaded < pw.len {
        if !state.client.choked {
            while state.backlog < MAX_BACKLOG && state.requested < pw.len {
                let block_size = MAX_BLOCK_SIZE.min(pw.len - state.requested);

                state
                    .client
                    .send_request(pw.index, state.requested, block_size)
                    .await?;
                state.backlog += 1;
                state.requested += block_size;
            }
        }
        state.read_msg().await?;
    }
    Ok(state.buf)
}

fn check_integrity(buf: &[u8], wrk: &PieceWork) -> bool {
    let hash = Sha1::from(buf).digest().bytes();
    hash == *wrk.hash.as_ref()
}

struct PieceProgress<'a, C> {
    index: usize,
    client: &'a mut Client<C>,
    buf: Vec<u8>,
    downloaded: usize,
    requested: usize,
    backlog: usize,
}

impl<C> PieceProgress<'_, C>
where
    C: AsyncRead + AsyncWrite + Unpin,
{
    async fn read_msg(&mut self) -> crate::Result<()> {
        let msg = match self.client.read().await? {
            Some(msg) => msg,
            // Keep-alive
            None => return Ok(()),
        };

        match msg.kind {
            MessageKind::Unchoke => self.client.choked = false,
            MessageKind::Choke => self.client.choked = true,
            MessageKind::Have => {
                let index = msg.parse_have()?;
                self.client.bitfield.set(index, true);
            }
            MessageKind::Piece => {
                let n = msg.parse_piece(self.index, &mut self.buf)?;
                self.downloaded += n;
                self.backlog -= 1;
            }
            _ => {}
        }

        Ok(())
    }
}

pub struct PieceIter<'a> {
    torrent: &'a Torrent,
    index: usize,
    count: usize,
}

impl<'a> PieceIter<'a> {
    fn new(torrent: &'a Torrent) -> Self {
        Self {
            torrent,
            index: 0,
            count: torrent.piece_hashes.len() / 20,
        }
    }
}

impl Iterator for PieceIter<'_> {
    type Item = PieceWork;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.count {
            return None;
        }

        let bytes = &self.torrent.piece_hashes[self.index * HASH_LEN..][..HASH_LEN];
        let hash = bytes.try_into().unwrap();

        let start = self.index * self.torrent.piece_len;
        let end = start + self.torrent.piece_len;

        let piece = PieceWork {
            index: self.index,
            len: end.min(self.torrent.length) - start,
            hash,
        };

        self.index += 1;

        Some(piece)
    }
}
