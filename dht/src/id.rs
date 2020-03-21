use data_encoding::HEXUPPER;
use rand::Rng;
use std::ops::BitXor;

#[derive(Clone, Debug, Default, PartialEq, PartialOrd, Eq, Ord)]
pub struct NodeId([u8; NodeId::LEN]);

impl NodeId {
    pub const LEN: usize = 20;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn of_byte(b: u8) -> Self {
        Self([b; Self::LEN])
    }

    pub fn random() -> Self {
        let mut n = Self::new();
        rand::thread_rng().fill(&mut n.0[..]);
        n
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

    pub fn dist(&self, other: &Self) -> Self {
        self ^ other
    }

    pub fn compare_ref(&self, n1: &Self, n2: &Self) -> bool {
        let lhs = self ^ n1;
        let rhs = self ^ n2;
        lhs < rhs
    }

    pub fn dist_exp(&self, other: &Self) -> usize {
        160 - self.dist(other).leading_zeros()
    }

    pub fn min_dist_exp(&self, ids: &[Self]) -> usize {
        debug_assert_ne!(ids.len(), 0);
        ids.iter().map(|id| self.dist_exp(id)).min().unwrap_or(160)
    }

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
    fn leading_zeros() {
        let a = NodeId::of_byte(0b0000_1000);
        assert_eq!(4, a.leading_zeros());
    }
}
