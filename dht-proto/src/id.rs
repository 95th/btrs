use ben::Encode;
use data_encoding::HEXUPPER_PERMISSIVE as hex;
use rand::distributions::uniform::{SampleBorrow, SampleUniform, UniformSampler};
use rand::Rng;
use std::fmt;
use std::ops::{BitAnd, BitXor, Deref, DerefMut};

#[derive(Copy, Clone, Default, PartialEq, PartialOrd, Eq, Ord, Hash)]
#[repr(transparent)]
pub struct NodeId([u8; 20]);

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.encode_hex())
    }
}

impl NodeId {
    pub const fn new() -> Self {
        Self::all(0)
    }

    pub const fn max() -> Self {
        Self::all(u8::max_value())
    }

    pub const fn all(b: u8) -> Self {
        Self([b; 20])
    }

    pub fn gen() -> Self {
        let mut id = Self::new();
        rand::thread_rng().fill(&mut id[..]);
        id
    }

    pub fn gen_leading_zeros(leading_zeros: usize) -> Self {
        Self::gen().mask_leading_zeros(leading_zeros)
    }

    pub fn from_hex(buf: &[u8]) -> anyhow::Result<Self> {
        let len = hex.decode_len(buf.len())?;
        anyhow::ensure!(len == 20, "Invalid hex for node ID");

        let mut id = Self::new();
        if let Err(e) = hex.decode_mut(buf, &mut id[..]) {
            anyhow::bail!("Unable to parse hex string: {:?}", e);
        }

        Ok(id)
    }

    pub fn is_zero(&self) -> bool {
        self.iter().all(|b| *b == 0)
    }

    pub fn encode_hex(&self) -> String {
        hex.encode(&self.0)
    }

    pub fn encode_base32(&self) -> String {
        data_encoding::BASE32.encode(&self.0)
    }

    /// Returns number of leading zeros.
    pub fn leading_zeros(&self) -> usize {
        let mut n = 0;
        for &c in self.iter() {
            if c == 0 {
                n += 8;
            } else {
                n += c.leading_zeros() as usize;
                break;
            }
        }
        n
    }

    /// Returns number of leading zeros of `XOR` of `self` with given `NodeId`
    pub fn xor_leading_zeros(&self, other: &Self) -> usize {
        (self ^ other).leading_zeros()
    }

    fn mask_leading_zeros(mut self, leading_zeros: usize) -> Self {
        if leading_zeros >= 160 {
            return Self::new();
        }

        for i in 0..leading_zeros / 8 {
            self[i] = 0;
        }

        if leading_zeros % 8 != 0 {
            let idx = leading_zeros / 8;
            self[idx] = 0xff >> (leading_zeros % 8);
        }

        self
    }
}

impl From<[u8; 20]> for NodeId {
    fn from(buf: [u8; 20]) -> Self {
        Self(buf)
    }
}

impl Deref for NodeId {
    type Target = [u8; 20];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for NodeId {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Encode for NodeId {
    fn encode(&self, buf: &mut Vec<u8>) {
        ben::encode_bytes(buf, &self[..]);
    }
}

macro_rules! bit_ops {
    ($from_ty:ty, $to_ty:ty) => {
        impl BitAnd<$from_ty> for $to_ty {
            type Output = NodeId;

            fn bitand(self, other: $from_ty) -> Self::Output {
                let mut id = NodeId::new();
                id.iter_mut()
                    .zip(self.iter().zip(other.iter()))
                    .for_each(|(a, (b, c))| *a = b & c);
                id
            }
        }

        impl BitXor<$from_ty> for $to_ty {
            type Output = NodeId;

            fn bitxor(self, other: $from_ty) -> NodeId {
                let mut id = NodeId::new();
                id.iter_mut()
                    .zip(self.iter().zip(other.iter()))
                    .for_each(|(a, (b, c))| *a = b ^ c);
                id
            }
        }
    };
}

bit_ops!(&NodeId, &NodeId);
bit_ops!(&NodeId, NodeId);
bit_ops!(NodeId, &NodeId);
bit_ops!(NodeId, NodeId);

impl SampleUniform for NodeId {
    type Sampler = UniformNodeId;
}

pub struct UniformNodeId {
    low: NodeId,
    high: NodeId,
    inclusive: bool,
}

impl UniformSampler for UniformNodeId {
    type X = NodeId;

    fn new<B1, B2>(low: B1, high: B2) -> Self
    where
        B1: SampleBorrow<Self::X> + Sized,
        B2: SampleBorrow<Self::X> + Sized,
    {
        let low = low.borrow();
        let high = high.borrow();
        assert!(low < high);

        UniformNodeId {
            low: *low,
            high: *high,
            inclusive: false,
        }
    }

    fn new_inclusive<B1, B2>(low: B1, high: B2) -> Self
    where
        B1: SampleBorrow<Self::X> + Sized,
        B2: SampleBorrow<Self::X> + Sized,
    {
        let low = low.borrow();
        let high = high.borrow();
        assert!(low <= high);

        UniformNodeId {
            low: *low,
            high: *high,
            inclusive: true,
        }
    }

    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> NodeId {
        let mut out = NodeId::new();
        let low_is_zero = self.low.is_zero();
        loop {
            rng.fill(&mut out[..]);
            if self.inclusive {
                if out <= self.high && (low_is_zero || out >= self.low) {
                    break out;
                }
            } else if out < self.high && (low_is_zero || out >= self.low) {
                break out;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_hex() {
        let h = b"3F3F3F3F3F3F3F3F3F3F3F3F3F3F3F3F3F3F3F3F";
        let n = NodeId::from_hex(h).unwrap();
        assert_eq!(NodeId::all(0x3F), n);
    }

    #[test]
    fn encode_hex() {
        let n = NodeId::all(0x3F);
        let s = n.encode_hex();
        assert_eq!("3F3F3F3F3F3F3F3F3F3F3F3F3F3F3F3F3F3F3F3F", s);
    }

    #[test]
    fn sort_order() {
        let mut a = [NodeId::all(0), NodeId::all(3), NodeId::all(1)];
        a.sort();
        assert_eq!([NodeId::all(0), NodeId::all(1), NodeId::all(3)], a);
    }

    #[test]
    fn xor() {
        let a = NodeId::all(0b0000_0101);
        let b = NodeId::all(0b1100_0100);
        let c = a ^ b;
        assert_eq!(NodeId::all(0b1100_0001), c);
    }

    #[test]
    fn test_gen_leading_zeros() {
        let n = NodeId::gen_leading_zeros(5);
        assert!(n.leading_zeros() >= 5);
    }

    #[test]
    fn test_mask_leading_zeros() {
        let actual = NodeId::max().mask_leading_zeros(5);
        let mut expected = NodeId::max();
        expected[0] = 0b0000_0111;
        assert_eq!(5, actual.leading_zeros());
        assert_eq!(expected, actual);

        let actual = NodeId::max().mask_leading_zeros(8);
        let mut expected = NodeId::max();
        expected[0] = 0;
        assert_eq!(8, actual.leading_zeros());
        assert_eq!(expected, actual);

        let actual = NodeId::max().mask_leading_zeros(9);
        let mut expected = NodeId::max();
        expected[0] = 0;
        expected[1] = 0b0111_1111;
        assert_eq!(9, actual.leading_zeros());
        assert_eq!(expected, actual);
    }
}
