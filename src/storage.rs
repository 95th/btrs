use crate::work::Piece;
use std::fs::File;
use std::io;

pub struct StorageWriter<'a, T> {
    piece_len: usize,
    inner: &'a mut T,
}

impl<'a, T: Storage> StorageWriter<'a, T> {
    pub fn new(inner: &'a mut T, piece_len: usize) -> Self {
        Self { inner, piece_len }
    }

    pub fn insert(&mut self, piece: Piece) -> io::Result<()> {
        trace!(
            "Writing index {}, {} bytes [piece len: {}, so pieces: {}]",
            piece.index,
            piece.buf.len(),
            self.piece_len,
            piece.buf.len() / self.piece_len
        );
        let offset = self.index_to_offset(piece.index);
        self.inner.write_all_at(&piece.buf, offset)?;
        Ok(())
    }

    fn index_to_offset(&self, index: u32) -> u64 {
        self.piece_len as u64 * index as u64
    }
}

/// Storage
pub trait Storage {
    /// Reads a number of bytes starting from a given offset.
    ///
    /// Returns the number of bytes read.
    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize>;

    /// Writes a number of bytes starting from a given offset.
    ///
    /// Returns the number of bytes written.
    fn write_at(&mut self, buf: &[u8], offset: u64) -> io::Result<usize>;

    fn read_exact_at(&self, mut buf: &mut [u8], mut offset: u64) -> io::Result<()> {
        while !buf.is_empty() {
            match self.read_at(buf, offset) {
                Ok(0) => break,
                Ok(n) => {
                    let tmp = buf;
                    buf = &mut tmp[n..];
                    offset += n as u64;
                }
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        if !buf.is_empty() {
            Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "failed to fill whole buffer",
            ))
        } else {
            Ok(())
        }
    }

    fn write_all_at(&mut self, mut buf: &[u8], mut offset: u64) -> io::Result<()> {
        while !buf.is_empty() {
            match self.write_at(buf, offset) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "failed to write whole buffer",
                    ));
                }
                Ok(n) => {
                    buf = &buf[n..];
                    offset += n as u64
                }
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

#[cfg(unix)]
impl Storage for File {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
        std::os::unix::fs::FileExt::read_at(self, buf, offset)
    }

    fn write_at(&mut self, buf: &[u8], offset: u64) -> io::Result<usize> {
        std::os::unix::fs::FileExt::write_at(self, buf, offset)
    }
}

#[cfg(windows)]
impl Storage for File {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
        std::os::windows::fs::FileExt::seek_read(self, buf, offset)
    }

    fn write_at(&mut self, buf: &[u8], offset: u64) -> io::Result<usize> {
        std::os::windows::fs::FileExt::seek_write(self, buf, offset)
    }
}

impl Storage for Vec<u8> {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
        let offset = offset as usize;
        if offset >= self.len() {
            return Ok(0);
        }

        let len = buf.len().min(self.len() - offset);

        buf[..len].copy_from_slice(&self[offset..][..len]);
        Ok(len)
    }

    fn write_at(&mut self, buf: &[u8], offset: u64) -> io::Result<usize> {
        let offset = offset as usize;
        if offset + buf.len() >= self.len() {
            self.resize(offset + buf.len(), 0);
        }

        self[offset..][..buf.len()].copy_from_slice(buf);
        Ok(buf.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;
    use std::fs::{File, OpenOptions};
    use std::str;

    macro_rules! check {
        ($e:expr) => {
            match $e {
                Ok(t) => t,
                Err(e) => panic!("{} failed with: {}", stringify!($e), e),
            }
        };
    }

    #[test]
    fn read_write_at() {
        let tmpdir = temp_dir();
        let filename = tmpdir.join("read_write_at.txt");
        let mut buf = [0; 256];
        let write1 = "asdf";
        let write2 = "qwer-";
        let write3 = "-zxcv";
        let content = "qwer-asdf-zxcv";
        {
            let oo = OpenOptions::new()
                .create_new(true)
                .write(true)
                .read(true)
                .clone();
            let mut rw = check!(oo.open(&filename));

            assert_eq!(check!(rw.write_at(write1.as_bytes(), 5)), write1.len());
            assert_eq!(check!(rw.read_at(&mut buf, 5)), write1.len());
            assert_eq!(str::from_utf8(&buf[..write1.len()]), Ok(write1));

            assert_eq!(
                check!(rw.read_at(&mut buf[..write2.len()], 0)),
                write2.len()
            );
            assert_eq!(str::from_utf8(&buf[..write2.len()]), Ok("\0\0\0\0\0"));

            assert_eq!(check!(rw.write_at(write2.as_bytes(), 0)), write2.len());
            assert_eq!(check!(rw.read_at(&mut buf, 5)), write1.len());
            assert_eq!(str::from_utf8(&buf[..write1.len()]), Ok(write1));

            assert_eq!(
                check!(rw.read_at(&mut buf[..write2.len()], 0)),
                write2.len()
            );
            assert_eq!(str::from_utf8(&buf[..write2.len()]), Ok(write2));
            assert_eq!(check!(rw.write_at(write3.as_bytes(), 9)), write3.len());
        }
        {
            let read = check!(File::open(&filename));

            assert_eq!(check!(read.read_at(&mut buf, 0)), content.len());
            assert_eq!(str::from_utf8(&buf[..content.len()]), Ok(content));

            assert_eq!(check!(read.read_at(&mut buf, 9)), write3.len());
            assert_eq!(str::from_utf8(&buf[..write3.len()]), Ok(write3));

            assert_eq!(check!(read.read_at(&mut buf, 14)), 0);
            assert_eq!(check!(read.read_at(&mut buf, 15)), 0);
        }
        check!(std::fs::remove_file(&filename));
    }
}
