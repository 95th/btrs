use crate::fs::FileExt;
use crate::work::Piece;
use std::collections::BinaryHeap;
use std::fs::File;
use std::io;

pub struct Cache<'a> {
    pieces: BinaryHeap<Piece>,
    piece_len: usize,
    total_len: usize,
    file: &'a File,
}

impl Cache<'_> {
    pub fn new(file: &File, capacity: usize, piece_len: usize, total_len: usize) -> Cache<'_> {
        Cache {
            file,
            piece_len,
            total_len,
            pieces: BinaryHeap::with_capacity(capacity),
        }
    }

    pub fn push(&mut self, piece: Piece) -> io::Result<()> {
        self.pieces.push(piece);

        if self.pieces.len() == self.pieces.capacity() {
            self.flush()?;
        }

        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        let Piece { mut index, mut buf } = self.pieces.pop().unwrap();
        while let Some(mut piece) = self.pieces.pop() {
            if piece.index == index + 1 {
                buf.append(&mut piece.buf);
                index += 1;
                continue;
            } else {
                let offset = self.index_to_offset(index);
                self.file.write_all_at(&buf, offset as u64)?;
                index = piece.index;
                buf = piece.buf;
            }
        }

        Ok(())
    }

    fn index_to_offset(&self, index: u32) -> usize {
        self.total_len.min(self.piece_len * index as usize)
    }
}
