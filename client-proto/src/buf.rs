use crate::avg::MovingAverage;

const MAX_BUF_SIZE: usize = 1024 * 1024;

pub struct RecvBuf {
    buf: Vec<u8>,
    write_pos: usize,
    read_pos: usize,
    write_rate: MovingAverage<5>,
    read_rate: MovingAverage<5>,
}

impl RecvBuf {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            write_pos: 0,
            read_pos: 0,
            write_rate: MovingAverage::new(),
            read_rate: MovingAverage::new(),
        }
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self {
            buf: vec![0; cap],
            ..Self::new()
        }
    }

    /// Reserve at least `len` unread bytes in the buffer and return a mutable reference
    /// to the unwritten region.
    ///
    /// If the `len` bytes are already buffered in this buffer, it will return an empty buffer.
    pub fn write_reserve(&mut self, len: usize) -> &mut [u8] {
        let unread = self.write_pos - self.read_pos;
        if unread >= len {
            return &mut [];
        }

        self.discard_read(len);

        if self.read_pos + len > self.buf.len() {
            let new_len = self.read_pos + len;
            self.buf.resize(new_len, 0);
        }

        &mut self.buf[self.write_pos..]
    }

    fn discard_read(&mut self, needed: usize) {
        if self.read_pos == 0 {
            return;
        }

        let unread = self.write_pos - self.read_pos;
        if unread == 0 {
            // Nothing is buffered. So just shift the pointers.
            self.write_pos -= self.read_pos;
            self.read_pos = 0;
            return;
        }

        if self.read_pos + needed <= self.buf.len() {
            // There is enough space for `needed` bytes. Do nothing.
            return;
        }

        // We dont have enough space. Discard the left side of the buffer.
        unsafe {
            let p = self.buf.as_mut_ptr();
            std::ptr::copy(p.add(self.read_pos), p, unread);
        }

        self.write_pos -= self.read_pos;
        self.read_pos = 0;
    }

    /// Advance the buffer's write cursor to denote that `n` bytes
    /// were successfully written to this buffer.
    pub fn advance_write(&mut self, n: usize) {
        self.write_pos += n;
        assert!(self.write_pos <= self.buf.len());

        self.write_rate.add_sample(n as isize);
        let write_rate = self.write_rate.mean() as usize;

        // If writes are filling atleast 90% of current buffer length at once,
        // increase the buffer length by 50%
        if write_rate >= self.buf.len() * 90 / 100 {
            let mut new_len = MAX_BUF_SIZE.min(self.buf.len() * 3 / 2);

            let read = self.read_rate.mean() as usize;
            if read > 0 {
                // Make the new length multiple of read rate so that there is
                // less copying when discarding read bytes
                new_len = read * ((new_len + read - 1) / read);
            }

            self.buf.resize(new_len, 0);
        }
    }

    /// Read one bytes from current read cursor position without advancing.
    pub fn peek(&self) -> u8 {
        assert!(self.read_pos < self.write_pos);
        self.buf[self.read_pos]
    }

    /// Read `n` bytes from current read cursor and advance the read
    /// cursor by `n` bytes and returns reference to an slice of `n` size.
    pub fn read(&mut self, n: usize) -> &[u8] {
        assert!(self.read_pos + n <= self.write_pos);
        let buf = &self.buf[self.read_pos..self.read_pos + n];
        self.read_pos += n;
        self.read_rate.add_sample(n as isize);
        buf
    }

    /// Read `N` bytes from current read cursor and advance the read
    /// cursor by `N` bytes and returns reference to an array of `N` size.
    pub fn read_array<const N: usize>(&mut self) -> &[u8; N] {
        let buf = self.read(N);
        unsafe { &*buf.as_ptr().cast() }
    }
}

#[cfg(test)]
mod tests {
    use super::RecvBuf;

    #[test]
    fn read_consumes_all_written() {
        let mut b = RecvBuf::new();
        let w = b.write_reserve(10);
        w[..8].fill(1);
        b.advance_write(8);

        assert_eq!(b.read(8), &[1; 8]);
        assert_eq!(b.read_pos, 8);
        assert_eq!(b.write_pos, 8);
    }

    #[test]
    fn read_space_is_discarded() {
        let mut b = RecvBuf::new();
        let w = b.write_reserve(10);
        w[..8].fill(1);
        b.advance_write(8);

        assert_eq!(b.read(8), &[1; 8]);
        assert_eq!(b.read_pos, 8);
        assert_eq!(b.write_pos, 8);

        b.write_reserve(3);
        assert_eq!(b.read_pos, 0);
        assert_eq!(b.write_pos, 0);
    }

    #[test]
    fn unread_data_is_preserved_and_buf_resizes() {
        let mut b = RecvBuf::new();
        let w = b.write_reserve(10);
        w[..8].fill(1);
        b.advance_write(8);

        assert_eq!(b.read_pos, 0);
        assert_eq!(b.write_pos, 8);
        assert_eq!(b.buf.len(), 10);

        b.write_reserve(11);
        assert_eq!(b.read_pos, 0);
        assert_eq!(b.write_pos, 8);
        assert_eq!(b.buf.len(), 11);
    }

    #[test]
    fn write_reserve_returns_empty_for_buffered_data() {
        let mut b = RecvBuf::new();
        let w = b.write_reserve(10);
        w[..8].fill(1);
        b.advance_write(8);

        assert_eq!(b.write_reserve(8), &mut []);
    }

    #[test]
    fn write_reserve_returns_mut_slice_for_partially_buffered_data() {
        let mut b = RecvBuf::new();
        let w = b.write_reserve(10);
        w[..8].fill(1);
        b.advance_write(8);

        assert_eq!(b.write_reserve(10).len(), 2);
    }

    #[test]
    fn read_array_advances_buf() {
        let mut b = RecvBuf::new();
        let w = b.write_reserve(10);
        w[..8].fill(1);
        b.advance_write(8);

        assert_eq!(b.read_array::<8>(), &[1; 8]);
        assert_eq!(b.read_pos, 8);
        assert_eq!(b.write_pos, 8);
    }

    #[test]
    #[should_panic]
    fn reading_more_than_buffered_panics() {
        let mut b = RecvBuf::new();
        b.write_reserve(8);
        b.advance_write(2);
        b.read(3);
    }

    #[test]
    fn peek_doesnt_consume() {
        let mut b = RecvBuf::new();
        let w = b.write_reserve(8);
        w[..8].fill(1);
        b.advance_write(8);

        assert_eq!(b.read_pos, 0);
        assert_eq!(b.peek(), 1);
        assert_eq!(b.read_pos, 0);
    }

    #[test]
    #[should_panic]
    fn peek_without_buffered_panics() {
        let mut b = RecvBuf::new();
        b.write_reserve(1);
        b.peek();
    }

    #[test]
    fn read_space_is_not_discarded_if_there_is_sufficient_space() {
        let mut b = RecvBuf::new();

        let w = b.write_reserve(10);
        w[..8].fill(1);
        b.advance_write(8);
        assert_eq!(b.write_pos, 8);

        assert_eq!(b.read_pos, 0);
        assert_eq!(b.read(7), &[1; 7]);
        assert_eq!(b.read_pos, 7);

        let w = b.write_reserve(3);
        w[..2].fill(2);
        b.advance_write(2);
        assert_eq!(b.write_pos, 10);

        assert_eq!(b.read_pos, 7);
        assert_eq!(b.read(3), &[1, 2, 2]);
        assert_eq!(b.read_pos, 10);
    }

    #[test]
    fn advance_within_buffer() {
        let mut b = RecvBuf::new();
        b.write_reserve(2);
        b.advance_write(2);
    }

    #[test]
    #[should_panic]
    fn advance_beyond_buffer_panics() {
        let mut b = RecvBuf::new();
        b.write_reserve(2);
        b.advance_write(3);
    }
}
