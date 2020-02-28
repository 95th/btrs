use std::ops::BitXor;

type Bytes = [u8; 20];

#[derive(Clone, Debug, Default, PartialEq, PartialOrd)]
pub struct NodeId(Bytes);

impl NodeId {
    pub fn dist(&self, other: &Self) -> Self {
        self ^ other
    }

    pub fn compare_ref(&self, n1: &Self, n2: &Self) -> bool {
        let lhs = self ^ n1;
        let rhs = self ^ n2;
        lhs < rhs
    }

    pub fn dist_exp(&self, other: &Self) -> u32 {
        160 - self.dist(other).leading_zeros()
    }

    pub fn min_dist_exp(&self, ids: &[Self]) -> u32 {
        ids.iter().map(|id| self.dist_exp(id)).min().unwrap_or(160)
    }

    pub fn leading_zeros(&self) -> u32 {
        let mut n = 0;
        for &c in self.0.iter() {
            if c == 0 {
                n += 8;
            } else {
                n += c.leading_zeros();
                break;
            }
        }
        n
    }
}

impl BitXor<&NodeId> for &NodeId {
    type Output = NodeId;

    fn bitxor(self, other: &NodeId) -> NodeId {
        let mut data = Bytes::default();
        data.iter_mut()
            .zip(self.0.iter().zip(other.0.iter()))
            .for_each(|(a, (b, c))| *a = b ^ c);
        NodeId(data)
    }
}
