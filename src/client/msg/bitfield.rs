use crate::bitfield::BitField;
use crate::client::msg::ids;
use crate::client::msg::Message;

pub struct BitFieldMessage {
    pub field: BitField,
}

impl BitFieldMessage {
    pub fn new(len: usize) -> Self {
        Self {
            field: BitField::new(len),
        }
    }
}

impl Message for BitFieldMessage {
    fn encode_buf(&self, buf: &mut [u8]) -> usize {
        let mut written = 0;
        let len = self.field.len_bytes();

        buf[written..][..4].copy_from_slice(&len.to_be_bytes());
        written += 4;

        buf[written] = ids::BIT_FIELD;
        written += 1;

        self.field.write_to(&mut buf[written..][..len]);
        written += len;

        assert_eq!(self.len(), written);
        written
    }

    fn len(&self) -> usize {
        self.field.len_bytes() + 5
    }

    fn decode(&mut self, buf: &[u8]) {
        self.field.copy_from(buf);
    }
}
