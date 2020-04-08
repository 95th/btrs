use ben::{Encode, Encoder};
use data_encoding::HEXUPPER;
use rand::distributions::uniform::{SampleBorrow, SampleUniform, UniformSampler};
use rand::Rng;
use std::ops::BitXor;

#[derive(Clone, Debug, Default, PartialEq, PartialOrd, Eq, Ord)]
pub struct NodeId([u8; NodeId::LEN]);

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

    pub fn decode_hex(hex: &[u8]) -> Result<Self, &'static str> {
        let mut n = Self::new();

        let len = HEXUPPER
            .decode_len(hex.len())
            .map_err(|_| "Invalid hex length for node ID")?;

        if len != n.0.len() {
            return Err("Invalid hex for node ID");
        }

        HEXUPPER
            .decode_mut(hex, &mut n.0)
            .map_err(|_| "Unable to parse hex string")?;

        Ok(n)
    }

    pub fn encode_hex(&self, buf: &mut [u8]) -> Result<(), &'static str> {
        let len = HEXUPPER.encode_len(self.0.len());

        if len != buf.len() {
            return Err("Invalid hex for node ID");
        }

        HEXUPPER.encode_mut(&self.0, buf);
        Ok(())
    }

    pub fn compare_ref(&self, n1: &Self, n2: &Self) -> bool {
        let lhs = self ^ n1;
        let rhs = self ^ n2;
        lhs < rhs
    }

    pub fn dist_exp(&self, other: &Self) -> usize {
        160 - self.xor_leading_zeros(other)
    }

    pub fn min_dist_exp(&self, ids: &[Self]) -> usize {
        debug_assert_ne!(ids.len(), 0);
        ids.iter().map(|id| self.dist_exp(id)).min().unwrap_or(160)
    }

    /// Returns number of leading zeros.
    ///
    /// # Usage:
    /// ```
    /// # use dht::id::NodeId;
    ///
    /// let id = NodeId::of_byte(0b0010_0010);
    ///
    /// assert_eq!(2, id.leading_zeros());
    /// ```
    pub fn leading_zeros(&self) -> usize {
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
    /// let id1 = NodeId::of_byte(0b0000_0101);
    /// let id2 = NodeId::of_byte(0b0010_0010);
    ///
    /// let n1 = id1.xor_leading_zeros(&id2);
    /// let n2 = (&id1 ^ &id2).leading_zeros();
    ///
    /// assert_eq!(n1, n2);
    /// ```
    pub fn xor_leading_zeros(&self, other: &Self) -> usize {
        (self ^ other).leading_zeros()
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
