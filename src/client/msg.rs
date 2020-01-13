use crate::bitfield::BitField;

pub trait Message {
    fn id(&self) -> u8;

    fn len(&self) -> usize;

    fn encode(&self) -> Vec<u8> {
        let mut buf = vec![0; self.len()];
        self.encode_buf(&mut buf, 0);
        buf
    }

    fn encode_buf(&self, buf: &mut [u8], offset: usize) -> usize;

    fn decode(&self, buf: &mut [u8], offset: usize, len: usize);
}

pub struct BitFieldMessage {
    pub bit_field: BitField,
}

impl BitFieldMessage {
    pub fn new(len: usize) -> Self {
        Self {
            bit_field: BitField::new(len),
        }
    }
}

impl Message for BitFieldMessage {
    fn encode_buf(&self, buf: &mut [u8], offset: usize) -> usize {
        let mut written = offset;

        buf[written..][..4].copy_from_slice(&self.bit_field.len().to_be_bytes());
        written += 4;

        buf[written] = self.id();
        written += 1;

        // self.bit_field.write_to(buf);
        written += self.bit_field.len();

        todo!()
    }

    fn len(&self) -> usize {
        unimplemented!()
    }

    fn decode(&self, buf: &mut [u8], offset: usize, len: usize) {
        unimplemented!()
    }

    fn id(&self) -> u8 {
        unimplemented!()
    }
}
