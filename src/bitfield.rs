use std::fmt;

#[derive(Default, Clone)]
pub struct BitField {
    arr: Box<[u8]>,
    len: usize,
}

impl fmt::Debug for BitField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bits").field("len", &self.len).finish()
    }
}

impl BitField {
    pub fn new(len: usize) -> Self {
        Self::with_value(len, false)
    }

    pub fn with_value(len: usize, val: bool) -> Self {
        let arr_len = (len as f64 / 8.0).ceil() as usize;
        let v = if val { !0 } else { 0 };
        let mut bits = Self {
            arr: vec![v; arr_len].into(),
            len,
        };
        bits.clear_unused();
        bits
    }

    pub fn get(&self, idx: usize) -> bool {
        assert!(idx < self.len);
        let i = idx / 8;
        let offset = idx % 8;
        (self.arr[i] & 1 << offset) != 0
    }

    pub fn set(&mut self, idx: usize, value: bool) -> bool {
        if idx >= self.len {
            return false;
        }
        let i = idx / 8;
        let offset = idx % 8;
        if value {
            self.arr[i] |= 1 << offset;
        } else {
            self.arr[i] &= !(1 << offset);
        }
        true
    }

    pub fn set_all(&mut self, value: bool) {
        if value {
            self.arr.iter_mut().for_each(|v| *v = !0);
            self.clear_unused();
        } else {
            self.arr.iter_mut().for_each(|v| *v = 0);
        }
    }

    pub fn all_true(&self) -> bool {
        if self.arr.is_empty() {
            return false;
        }

        for i in 0..self.arr.len() - 1 {
            if self.arr[i] != !0 {
                return false;
            }
        }

        let remaining = self.len - (self.arr.len() - 1) * 8;
        let last_byte: u8 = self.arr[self.arr.len() - 1] << (8 - remaining);
        last_byte.count_ones() as usize == remaining
    }

    pub fn true_count(&self) -> usize {
        if self.arr.is_empty() {
            return 0;
        }

        let mut count = 0;
        for i in 0..self.arr.len() - 1 {
            count += self.arr[i].count_ones() as usize;
        }

        let remaining = self.len - (self.arr.len() - 1) * 8;
        let last_byte: u8 = self.arr[self.arr.len() - 1] << (8 - remaining);
        count + last_byte.count_ones() as usize
    }

    pub fn iter(&self) -> BitIter {
        BitIter {
            field: self,
            idx: 0,
        }
    }

    pub fn to_vec(&self) -> Vec<bool> {
        self.iter().collect()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.arr.iter().all(|&b| b == 0)
    }

    pub fn len_bytes(&self) -> usize {
        self.arr.len()
    }

    pub fn write_to(&self, buf: &mut [u8]) {
        buf.copy_from_slice(&self.arr);
    }

    pub fn copy_from(&mut self, buf: &[u8]) {
        self.arr.copy_from_slice(&buf[..self.arr.len()]);
        self.clear_unused();
    }

    fn clear_unused(&mut self) {
        if self.arr.is_empty() {
            return;
        }

        let extra_bits = (self.arr.len() * 8) - self.len;
        let last_byte = &mut self.arr[self.arr.len() - 1];
        for i in 0..extra_bits {
            let mask: u8 = 1 << (7 - i);
            *last_byte &= !mask;
        }
    }
}

impl From<Vec<u8>> for BitField {
    fn from(buf: Vec<u8>) -> Self {
        let len = buf.len() * 8;
        Self {
            arr: buf.into(),
            len,
        }
    }
}

pub struct BitIter<'a> {
    field: &'a BitField,
    idx: usize,
}

impl Iterator for BitIter<'_> {
    type Item = bool;

    fn next(&mut self) -> Option<bool> {
        if self.idx == self.field.len {
            None
        } else {
            let value = self.field.get(self.idx);
            self.idx += 1;
            Some(value)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iter_01() {
        let f = BitField::new(3);
        assert_eq!(vec![false, false, false], f.to_vec());
    }

    #[test]
    fn iter_02() {
        let mut f = BitField::new(3);
        assert!(f.set(0, true));
        assert!(f.set(2, true));
        assert_eq!(vec![true, false, true], f.to_vec());

        assert!(f.set(1, true));
        assert_eq!(vec![true, true, true], f.to_vec());

        for i in 0..3 {
            assert!(f.set(i, false));
        }
        assert_eq!(vec![false, false, false], f.to_vec());

        assert!(!f.set(3, false));
    }

    #[test]
    fn get_01() {
        let mut f = BitField::new(3);
        assert!(f.set(0, true));
        assert!(f.set(2, true));
        assert_eq!(true, f.get(0));
        assert_eq!(false, f.get(1));
        assert_eq!(true, f.get(2));
    }

    #[test]
    #[should_panic]
    fn get_02() {
        let f = BitField::new(3);
        f.get(3);
    }

    #[test]
    fn all_true_and_true_count() {
        let mut f = BitField::new(3);
        assert!(f.set(0, true));
        assert!(f.set(2, true));
        assert_eq!(false, f.all_true());
        assert_eq!(2, f.true_count());

        assert!(f.set(1, true));
        assert_eq!(true, f.all_true());
        assert_eq!(3, f.true_count());
    }

    #[test]
    fn set_all_with_all_true_and_true_count() {
        let mut f = BitField::new(3);

        f.set_all(true);
        assert_eq!(true, f.all_true());
        assert_eq!(3, f.true_count());

        f.set(2, false);
        assert_eq!(false, f.all_true());
        assert_eq!(2, f.true_count());
    }

    #[test]
    fn clear_unused() {
        let mut f = BitField::new(17);
        assert_eq!(3, f.len_bytes());
        assert_eq!(17, f.len());

        f.set_all(true);
        assert_eq!(&[0xff, 0xff, 0x01], &f.arr[..]);
        f.set_all(false);
        assert_eq!(&[0x00, 0x00, 0x00], &f.arr[..]);
        assert!(!f.set(21, true));
        assert_eq!(&[0x00, 0x00, 0x00], &f.arr[..]);
    }
}
