use std::fmt::Write;

#[derive(Debug, Default, PartialEq)]
pub struct InfoHash([u8; 20]);

impl InfoHash {
    pub fn encode_url(&self) -> String {
        let mut encoded = String::new();
        for &c in &self.0 {
            write!(&mut encoded, "%{:02X}", c).unwrap();
        }
        encoded
    }
    pub fn encode_hex(&self) -> String {
        data_encoding::HEXUPPER_PERMISSIVE.encode(&self.0)
    }

    pub fn encode_base32(&self) -> String {
        data_encoding::BASE32.encode(&self.0)
    }

    pub fn as_mut(&mut self) -> &mut [u8; 20] {
        &mut self.0
    }
}

impl From<[u8; 20]> for InfoHash {
    fn from(b: [u8; 20]) -> Self {
        Self(b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple() {
        assert_eq!(
            "%01%01%01%01%01%01%01%01%01%01%01%01%01%01%01%01%01%01%01%01",
            InfoHash([1; 20]).encode_url()
        );
        assert_eq!(
            "%0C%0C%0C%0C%0C%0C%0C%0C%0C%0C%0C%0C%0C%0C%0C%0C%0C%0C%0C%0C",
            InfoHash([0x0C; 20]).encode_url()
        );
    }

    #[test]
    fn test_value() {
        let i = InfoHash([
            0x86, 0xd4, 0xc8, 0x00, 0x24, 0xa4, 0x69, 0xbe, 0x4c, 0x50, 0xbc, 0x5a, 0x10, 0x2c,
            0xf7, 0x17, 0x80, 0x31, 0x00, 0x74,
        ]);
        assert_eq!(
            "%86%D4%C8%00%24%A4%69%BE%4C%50%BC%5A%10%2C%F7%17%80%31%00%74",
            i.encode_url()
        );
    }
}
