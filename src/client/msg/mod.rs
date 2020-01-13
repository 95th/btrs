pub trait Message {
    fn len(&self) -> usize;

    fn encode(&self) -> Vec<u8> {
        let mut buf = vec![0; self.len()];
        self.encode_buf(&mut buf);
        buf
    }

    fn encode_buf(&self, buf: &mut [u8]) -> usize;

    fn decode(&mut self, buf: &[u8]);
}

mod ids {
    pub const BIT_FIELD: u8 = 5;
}

pub mod bitfield;
