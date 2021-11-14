use crate::avg::MovingAverage;

const MAX_BUF_SIZE: usize = 1024 * 1024;

pub struct RecvBuffer {
    buf: Vec<u8>,
    write_pos: usize,
    read_pos: usize,
    write_rate: MovingAverage<5>,
    read_rate: MovingAverage<5>,
}

impl RecvBuffer {
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
    /// If the `len` bytes are already written to this buffer, it will return an empty buffer.
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
            if unread > self.read_pos {
                // There is overlap - use memmove
                std::ptr::copy(p.add(self.read_pos), p, unread);
            } else {
                // Otherwise memcpy
                std::ptr::copy_nonoverlapping(p.add(self.read_pos), p, unread);
            }
        }

        self.write_pos -= self.read_pos;
        self.read_pos = 0;
    }

    /// Advance the buffer's write cursor to denote that `n` bytes
    /// were successfully written to this buffer.
    pub fn advance_write(&mut self, n: usize) {
        self.write_pos += n;

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
        self.buf[self.read_pos]
    }

    /// Read `n` bytes from current read cursor and advance the read
    /// cursor by `n` bytes and returns reference to an slice of `n` size.
    pub fn read(&mut self, n: usize) -> &[u8] {
        self.read_rate.add_sample(n as isize);
        let buf = &self.buf[self.read_pos..self.read_pos + n];
        self.read_pos += n;
        buf
    }

    /// Read `N` bytes from current read cursor and advance the read
    /// cursor by `N` bytes and returns reference to an array of `N` size.
    pub fn read_array<const N: usize>(&mut self) -> &[u8; N] {
        let b = self.read(N);
        unsafe { &*b.as_ptr().cast() }
    }
}
