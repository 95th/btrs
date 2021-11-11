use crate::avg::SlidingAvg;
use crate::future::timeout;
use crate::work::{Piece, PieceInfo, WorkQueue};
use anyhow::Context;
use client::msg::{Packet, PieceBlock};
use client::{AsyncStream, Client};
use futures::channel::mpsc::Sender;
use futures::SinkExt;
use std::collections::HashMap;
use std::time::Instant;

const MAX_REQUESTS: u32 = 500;
const MIN_REQUESTS: u32 = 2;
const MAX_BLOCK_SIZE: u32 = 0x4000;

struct PieceInProgress {
    piece: PieceInfo,
    buf: Box<[u8]>,
    downloaded: u32,
    requested: u32,
}

pub struct Download<'w, C> {
    /// Peer connection
    client: Client<C>,

    /// Common work queue from where we pick the pieces to download
    work: &'w WorkQueue,

    /// Channel to send the completed and verified pieces
    piece_tx: Sender<Piece>,

    /// In-progress pieces
    in_progress: HashMap<u32, PieceInProgress>,

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

    recv_buf: Vec<u8>,
}

impl<C> Drop for Download<'_, C> {
    fn drop(&mut self) {
        // Put any unfinished pieces back in the work queue
        self.work
            .extend(self.in_progress.drain().map(|(_i, p)| p.piece));
    }
}

impl<'w, C: AsyncStream> Download<'w, C> {
    pub async fn new(
        mut client: Client<C>,
        work: &'w WorkQueue,
        piece_tx: Sender<Piece>,
    ) -> anyhow::Result<Download<'w, C>> {
        client.send_unchoke();
        client.send_interested();
        client.flush().await?;

        let mut recv_buf = Vec::new();
        client.wait_for_unchoke(&mut recv_buf).await?;

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
            recv_buf,
        })
    }

    pub async fn start(&mut self) -> anyhow::Result<()> {
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

    async fn handle_msg(&mut self) -> anyhow::Result<()> {
        let PieceBlock { begin, index, data } = loop {
            let packet = self.client.read_packet(&mut self.recv_buf).await?;
            if let Some(Packet::Piece(p)) = packet {
                break p;
            }
        };

        let mut p = self
            .in_progress
            .remove(&index)
            .context("Received a piece that was not requested")?;

        p.buf[begin as usize..][..data.len()].copy_from_slice(data);
        p.downloaded += data.len() as u32;
        self.work.add_downloaded(data.len());
        self.backlog -= 1;
        trace!("current index {}: {}/{}", index, p.downloaded, p.piece.len);

        if p.downloaded < p.piece.len {
            // Not done yet
            self.in_progress.insert(index, p);
            return Ok(());
        }

        self.piece_done(p).await
    }

    async fn piece_done(&mut self, state: PieceInProgress) -> anyhow::Result<()> {
        trace!("Piece downloaded: {}", state.piece.index);
        let verified = self.work.verify(&state.piece, &state.buf).await;

        if !verified {
            error!("Bad piece: Hash mismatch for {}", state.piece.index);
            self.work.add_piece(state.piece);
            return Ok(());
        }

        info!("Downloaded and Verified {} piece", state.piece.index);
        self.client.send_have(state.piece.index);
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

        if let Some(piece) = self.work.remove_piece() {
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
        if self.client.is_choked() || self.backlog >= MIN_REQUESTS {
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
                self.client
                    .send_request(s.piece.index, s.requested, block_size);

                self.backlog += 1;
                s.requested += block_size;
                need_flush = true;
            }
        }

        if need_flush {
            self.last_requested_blocks = self.backlog;
            self.last_requested = Instant::now();

            trace!("Flushing the client");
            timeout(self.client.flush(), 5).await
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
