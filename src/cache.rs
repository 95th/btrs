use crate::fs::FileExt;
use crate::work::Piece;
use log::debug;
use std::collections::BinaryHeap;
use std::fs::File;
use std::io;

pub struct Cache<'a> {
    pieces: BinaryHeap<Piece>,
    piece_len: usize,
    total_len: usize,
    limit: usize,
    file: &'a File,
}

impl Cache<'_> {
    pub fn new(file: &File, limit: usize, piece_len: usize, total_len: usize) -> Cache<'_> {
        Cache {
            file,
            piece_len,
            total_len,
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
        let mut last = match self.pieces.pop() {
            Some(p) => p,
            None => return Ok(()),
        };

        while let Some(mut piece) = self.pieces.pop() {
            if piece.index == last.index + 1 {
                last.buf.append(&mut piece.buf);
                last.index += 1;
                continue;
            } else {
                debug!(
                    "Writing index {}, {} bytes [piece len: {}, so pieces: {}]",
                    last.index,
                    last.buf.len(),
                    self.piece_len,
                    last.buf.len() / self.piece_len
                );
                let offset = self.index_to_offset(last.index);
                self.file.write_all_at(&last.buf, offset as u64)?;
                last = piece;
            }
        }

        debug!(
            "Writing index {}, {} bytes [piece len: {}, so pieces: {}]",
            last.index,
            last.buf.len(),
            self.piece_len,
            last.buf.len() / self.piece_len
        );
        let offset = self.index_to_offset(last.index);
        self.file.write_all_at(&last.buf, offset as u64)?;

        Ok(())
    }

    fn index_to_offset(&self, index: u32) -> usize {
        self.total_len.min(self.piece_len * index as usize)
    }
}
