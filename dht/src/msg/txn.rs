use ben::{Encode, Encoder};

#[derive(Debug, Copy, Clone, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct TxnId(pub u16);

impl Encode for TxnId {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        enc.add_bytes(&self.0.to_be_bytes()[..]);
    }
}
