use percent_encoding::{percent_encode, PercentEncode, NON_ALPHANUMERIC};
use std::convert::TryFrom;

type Bytes = [u8; 20];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InfoHash(Bytes);

impl InfoHash {
    pub fn encode_url(&self) -> PercentEncode {
        percent_encode(&self.0, NON_ALPHANUMERIC)
    }

    pub fn encode_hex(&self) -> String {
        data_encoding::HEXUPPER_PERMISSIVE.encode(&self.0)
    }

    pub fn encode_base32(&self) -> String {
        data_encoding::BASE32.encode(&self.0)
    }
}

impl AsRef<Bytes> for InfoHash {
    fn as_ref(&self) -> &Bytes {
        &self.0
    }
}

impl AsMut<Bytes> for InfoHash {
    fn as_mut(&mut self) -> &mut Bytes {
        &mut self.0
    }
}

impl From<Bytes> for InfoHash {
    fn from(b: Bytes) -> Self {
        Self(b)
    }
}

impl TryFrom<&[u8]> for InfoHash {
    type Error = &'static str;

    fn try_from(buf: &[u8]) -> Result<Self, Self::Error> {
        let buf = Bytes::try_from(buf).map_err(|_| "Incorrect Infohash length")?;
        Ok(Self(buf))
    }
}
