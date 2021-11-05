use std::fmt;

#[derive(Default, Clone)]
pub struct Bitfield {
    buf: Vec<u32>,
    bits: usize,
}

impl fmt::Debug for Bitfield {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bitfield")
            .field("bits", &self.bits)
            .finish()
    }
}

impl Bitfield {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            bits: 0,
        }
    }

    pub fn with_size(bits: usize) -> Self {
        Self::with_value(bits, false)
    }

    pub fn with_value(bits: usize, value: bool) -> Self {
        let mut bitfield = Self::new();
        bitfield.resize(bits);
        if value {
            bitfield.set_all();
        }
        bitfield
    }

    pub fn as_bytes(&self) -> &[u8] {
        let ptr = self.buf.as_ptr().cast();
        unsafe { std::slice::from_raw_parts(ptr, self.len_bytes()) }
    }

    pub fn copy_from_slice(&mut self, bits: usize, buf: &[u8]) {
        self.resize(bits);
        assert_eq!(buf.len(), self.len_bytes());
        unsafe {
            let ptr = self.buf.as_mut_ptr().cast();
            std::ptr::copy_nonoverlapping(buf.as_ptr(), ptr, self.len_bytes());
        }
        self.clear_trailing_bits();
    }

    pub fn len(&self) -> usize {
        self.bits as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len_bytes(&self) -> usize {
        (self.bits + 7) / 8
    }

    pub fn get_bit(&self, index: usize) -> bool {
        if let Some(word) = self.word(index) {
            word & mask(index) != 0
        } else {
            false
        }
    }

    pub fn set_bit(&mut self, index: usize) {
        if let Some(v) = self.word_mut(index) {
            *v |= mask(index);
        }
    }

    pub fn clear_bit(&mut self, index: usize) {
        if let Some(v) = self.word_mut(index) {
            *v &= !mask(index);
        }
    }

    pub fn set_all(&mut self) {
        self.buf.fill(u32::MAX);
        self.clear_trailing_bits();
    }

    pub fn clear_all(&mut self) {
        self.buf.fill(0);
    }

    pub fn is_all_set(&self) -> bool {
        let words = self.buf.len().min(self.bits / 32);

        for i in 0..words {
            if self.buf[i] != u32::MAX {
                return false;
            }
        }

        if words < self.buf.len() {
            let bits = self.bits % 32;
            let mask = (u32::MAX << (32 - bits)).to_be();
            return self.buf[words] & mask == mask;
        }

        true
    }

    pub fn count(&self) -> usize {
        self.buf.iter().map(|n| n.count_ones() as usize).sum()
    }

    pub fn clear(&mut self) {
        self.buf.clear();
        self.bits = 0;
    }

    pub fn iter(&self) -> Iter<'_> {
        Iter {
            bitfield: self,
            index: 0,
        }
    }

    pub fn resize(&mut self, bits: usize) {
        let words = (31 + self.bits) / 32;
        let new_words = (31 + bits) / 32;

        if words != new_words {
            self.buf.resize(new_words, 0);
        }

        self.bits = bits;
        self.clear_trailing_bits();
    }

    fn clear_trailing_bits(&mut self) {
        let bits = self.bits % 32;

        if bits == 0 {
            return;
        }

        if let Some(v) = self.buf.last_mut() {
            let mask = u32::MAX << (32 - bits as u32);
            *v &= mask.to_be();
        }
    }

    fn word(&self, index: usize) -> Option<u32> {
        if index < self.bits {
            self.buf.get(index / 32).copied()
        } else {
            None
        }
    }

    fn word_mut(&mut self, index: usize) -> Option<&mut u32> {
        if index < self.bits {
            self.buf.get_mut(index / 32)
        } else {
            None
        }
    }
}

fn mask(index: usize) -> u32 {
    let m = 0x8000_0000_u32 >> (index % 32);
    m.to_be()
}

pub struct Iter<'a> {
    bitfield: &'a Bitfield,
    index: usize,
}

impl Iterator for Iter<'_> {
    type Item = bool;

    fn next(&mut self) -> Option<bool> {
        if self.index >= self.bitfield.bits {
            return None;
        }

        let value = self.bitfield.get_bit(self.index);
        self.index += 1;
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_bit() {
        let mut b = Bitfield::with_size(16);
        b.set_bit(4);
        b.set_bit(14);
        assert_eq!(b.as_bytes(), &[0b0000_1000, 0b0000_0010]);
    }

    #[test]
    fn get_bit() {
        let mut b = Bitfield::with_size(16);
        b.set_bit(4);
        b.set_bit(14);
        assert_eq!(b.get_bit(2), false);
        assert_eq!(b.get_bit(4), true);
        assert_eq!(b.get_bit(17), false);
    }

    #[test]
    fn clear_bit() {
        let mut b = Bitfield::with_size(16);
        b.set_bit(4);
        assert_eq!(b.get_bit(4), true);
        b.clear_bit(4);
        assert_eq!(b.get_bit(4), false);
    }

    #[test]
    fn count() {
        let mut b = Bitfield::with_size(16);
        assert_eq!(b.count(), 0);
        b.set_bit(4);
        b.set_bit(14);
        assert_eq!(b.count(), 2);
        b.set_bit(14);
        assert_eq!(b.count(), 2);
    }

    #[test]
    fn len() {
        let b = Bitfield::with_size(16);
        assert_eq!(b.len(), 16);
        assert_eq!(b.len_bytes(), 2);
    }

    #[test]
    fn set_all() {
        let mut b = Bitfield::with_size(16);
        b.set_all();
        assert_eq!(b.count(), 16);
        assert!(b.is_all_set());
    }

    #[test]
    fn clear() {
        let mut b = Bitfield::with_size(16);
        b.clear();
        assert!(b.is_empty());
    }

    #[test]
    fn resize_larger() {
        let mut b = Bitfield::with_size(16);
        b.set_bit(5);
        assert_eq!(b.get_bit(5), true);

        b.resize(128);
        assert_eq!(b.get_bit(5), true);
    }

    #[test]
    fn resize_smaller() {
        let mut b = Bitfield::with_size(128);
        b.set_bit(5);
        assert_eq!(b.get_bit(5), true);

        b.resize(8);
        assert_eq!(b.get_bit(5), true);

        b.resize(4);
        assert_eq!(b.get_bit(5), false);
    }

    #[test]
    fn clear_all() {
        let mut b = Bitfield::with_size(128);
        b.set_bit(5);
        b.set_bit(45);
        assert_eq!(b.count(), 2);

        b.clear_all();
        assert_eq!(b.count(), 0);
    }

    #[test]
    fn copy_from_slice() {
        let mut b = Bitfield::with_size(16);
        assert_eq!(b.len(), 16);

        b.copy_from_slice(20, &[0xff, 0xff, 0xff]);
        assert_eq!(b.count(), 20);
        assert_eq!(b.as_bytes(), &[0xff, 0xff, 0xf0]);
    }
}
