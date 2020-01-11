type Bytes = [u8; 20];

#[derive(Debug, Default, PartialEq)]
pub struct InfoHash(Bytes);

impl InfoHash {
    pub fn encode_url(&self) -> String {
        const HEX_CHARS: &[u8] = b"0123456789ABCDEF";
        let mut encoded = String::new();
        for &c in &self.0 {
            match c {
                b' ' => encoded.push('+'),
                c if c.is_ascii_alphanumeric() => encoded.push(c as char),
                c => {
                    encoded.push('%');
                    encoded.push(HEX_CHARS[(c >> 4) as usize] as char);
                    encoded.push(HEX_CHARS[(c & 0xf) as usize] as char);
                }
            }
        }
        encoded
    }

    pub fn encode_hex(&self) -> String {
        data_encoding::HEXUPPER_PERMISSIVE.encode(&self.0)
    }

    pub fn encode_base32(&self) -> String {
        data_encoding::BASE32.encode(&self.0)
    }

    pub fn as_mut(&mut self) -> &mut Bytes {
        &mut self.0
    }
}

impl From<Bytes> for InfoHash {
    fn from(b: Bytes) -> Self {
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
            "%86%D4%C8%00%24%A4i%BELP%BCZ%10%2C%F7%17%801%00t",
            i.encode_url()
        );
    }
}
