use ben::{Encode, Encoder};
use data_encoding::HEXUPPER_PERMISSIVE as hex_decoder;
use rand::distributions::uniform::{SampleBorrow, SampleUniform, UniformSampler};
use rand::Rng;
use std::fmt;
use std::ops::BitXor;

#[derive(Clone, Default, PartialEq, PartialOrd, Eq, Ord)]
#[repr(transparent)]
pub struct NodeId(pub [u8; NodeId::LEN]);

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl NodeId {
    pub const LEN: usize = 20;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn max() -> Self {
        Self::of_byte(u8::max_value())
    }

    pub fn of_byte(b: u8) -> Self {
        Self([b; Self::LEN])
    }

    pub fn is_zero(&self) -> bool {
        self.0.iter().all(|b| *b == 0)
    }

    pub fn gen() -> Self {
        let mut n = Self::new();
        rand::thread_rng().fill(&mut n.0[..]);
        n
    }

    pub fn gen_range(lo: &Self, hi: &Self) -> Self {
        rand::thread_rng().gen_range(lo, hi)
    }

    pub fn decode_hex(hex: &[u8]) -> anyhow::Result<Self> {
        let len = hex_decoder.decode_len(hex.len())?;
        ensure!(len == Self::LEN, "Invalid hex for node ID");

        let mut n = Self::new();
        if let Err(e) = hex_decoder.decode_mut(hex, &mut n.0) {
            bail!("Unable to parse hex string: {:?}", e);
        }

        Ok(n)
    }

    pub fn encode_hex(&self, buf: &mut [u8]) -> anyhow::Result<()> {
        let len = hex_decoder.encode_len(self.0.len());
        ensure!(len == buf.len(), "Invalid hex for node ID");

        hex_decoder.encode_mut(&self.0, buf);
        Ok(())
    }

    /// Returns number of leading zeros.
    ///
    /// # Usage:
    /// ```
    /// # use dht::id::NodeId;
    ///
    /// let id = NodeId::of_byte(0b0010_0010);
    ///
    /// assert_eq!(2, id.lz());
    /// ```
    pub fn lz(&self) -> usize {
        let mut n = 0;
        for &c in self.0.iter() {
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
    ///
    /// # Usage:
    /// ```
    /// # use dht::id::NodeId;
    ///
    /// let id1 = &NodeId::of_byte(0b0000_0101);
    /// let id2 = &NodeId::of_byte(0b0010_0010);
    ///
    /// let n1 = id1.xlz(id2);
    /// let n2 = (id1 ^ id2).lz();
    ///
    /// assert_eq!(n1, n2);
    /// ```
    pub fn xlz(&self, other: &Self) -> usize {
        (self ^ other).lz()
    }

    pub fn as_bytes(&self) -> &[u8; Self::LEN] {
        &self.0
    }
}

impl Encode for NodeId {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        enc.add_bytes(&self.0[..]);
    }
}

impl BitXor<&NodeId> for &NodeId {
    type Output = NodeId;

    fn bitxor(self, other: &NodeId) -> NodeId {
        let mut n = NodeId::default();
        n.0.iter_mut()
            .zip(self.0.iter().zip(other.0.iter()))
            .for_each(|(a, (b, c))| *a = b ^ c);
        n
    }
}

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
            low: low.clone(),
            high: high.clone(),
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
            low: low.clone(),
            high: high.clone(),
            inclusive: true,
        }
    }

    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> NodeId {
        let mut out = NodeId::new();
        let low_is_zero = self.low.is_zero();
        loop {
            rng.fill(&mut out.0);
            if self.inclusive {
                if out <= self.high && (low_is_zero || out >= self.low) {
                    break out;
                }
            } else {
                if out < self.high && (low_is_zero || out >= self.low) {
                    break out;
                }
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
        let n = NodeId::decode_hex(h).unwrap();
        assert_eq!(NodeId::of_byte(0x3F), n);
    }

    #[test]
    fn encode_hex() {
        let n = NodeId::of_byte(0x3F);
        let mut buf = [0; NodeId::LEN * 2];
        n.encode_hex(&mut buf).unwrap();
        assert_eq!(b"3F3F3F3F3F3F3F3F3F3F3F3F3F3F3F3F3F3F3F3F"[..], buf[..]);
    }

    #[test]
    fn sort_order() {
        let mut a = [NodeId::of_byte(0), NodeId::of_byte(3), NodeId::of_byte(1)];
        a.sort();
        assert_eq!(
            [NodeId::of_byte(0), NodeId::of_byte(1), NodeId::of_byte(3)],
            a
        );
    }

    #[test]
    fn xor() {
        let a = NodeId::of_byte(0b0000_0101);
        let b = NodeId::of_byte(0b1100_0100);
        let c = &a ^ &b;
        assert_eq!(NodeId::of_byte(0b1100_0001), c);
    }

    #[test]
    fn test_gen_range() {
        let a = NodeId::of_byte(0);
        let b = NodeId::of_byte(5);
        let c = NodeId::gen_range(&a, &b);
        assert!(c >= a);
        assert!(c < b);
    }
}
