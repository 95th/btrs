use crate::fs::FileExt;
use crate::work::Piece;
use log::{debug, trace};
use std::collections::BinaryHeap;
use std::fs::File;
use std::io;

pub struct Cache<'a> {
    pieces: BinaryHeap<Piece>,
    piece_len: usize,
    limit: usize,
    file: &'a File,
}

impl Cache<'_> {
    pub fn new(file: &File, limit: usize, piece_len: usize) -> Cache<'_> {
        Cache {
            file,
            piece_len,
            limit,
            pieces: BinaryHeap::with_capacity(limit),
        }
    }

    pub fn push(&mut self, piece: Piece) -> io::Result<()> {
        self.pieces.push(piece);

        if self.pieces.len() >= self.limit {
            self.flush()?;
        }

        Ok(())
    }

    pub fn flush(&mut self) -> io::Result<()> {
        debug!("Start flush: {}", self.pieces.len());
        let mut last = match self.pieces.pop() {
            Some(p) => p,
            None => return Ok(()),
        };
        let mut curr_idx = last.index;

        while let Some(mut piece) = self.pieces.pop() {
            curr_idx += 1;
            if piece.index == curr_idx {
                last.buf.append(&mut piece.buf);
                continue;
            } else {
                trace!(
                    "Writing index {}, {} bytes [piece len: {}, so pieces: {}]",
                    last.index,
                    last.buf.len(),
                    self.piece_len,
                    last.buf.len() / self.piece_len
                );
                let offset = self.index_to_offset(last.index);
                self.file.write_all_at(&last.buf, offset as u64)?;
                last = piece;
                curr_idx = last.index;
            }
        }

        trace!(
            "Writing index {}, {} bytes [piece len: {}, so pieces: {}]",
            last.index,
            last.buf.len(),
            self.piece_len,
            last.buf.len() / self.piece_len
        );
        let offset = self.index_to_offset(last.index);
        self.file.write_all_at(&last.buf, offset)?;

        debug!("End flush: {}", self.pieces.len());
        Ok(())
    }

    fn index_to_offset(&self, index: u32) -> u64 {
        self.piece_len as u64 * index as u64
    }
}
