use anyhow::Context;
use ben::{DictEncoder, Encode, Entry, Parser};

const METADATA_PIECE_LEN: usize = 0x4000;

pub struct ExtendedMessage<'a, 'p> {
    pub id: u8,
    pub value: Entry<'a, 'p>,
    pub rest: &'a [u8],
}

mod msg_type {
    pub const REQUEST: i64 = 0;
    pub const DATA: i64 = 1;
    pub const REJECT: i64 = 2;
}

impl<'a, 'p> ExtendedMessage<'a, 'p> {
    pub fn parse(data: &'a [u8], parser: &'p mut Parser) -> anyhow::Result<Self> {
        let id = data[0];
        let (value, i) = parser.parse_prefix::<Entry>(&data[1..])?;
        log::debug!("ext header len: {}", value.as_raw_bytes().len());

        let rest = &data[i + 1..];
        log::debug!("ext data len: {}", rest.len());
        Ok(Self { id, value, rest })
    }

    pub fn is_handshake(&self) -> bool {
        self.id == 0
    }

    pub fn body(&self) -> &Entry<'a, 'p> {
        &self.value
    }

    pub fn metadata(&self) -> Option<Metadata> {
        log::trace!("metadata: {:#?}", self.value);
        let dict = self.value.as_dict()?;
        let m = dict.get_dict("m")?;
        let id = m.get_int("ut_metadata")? as u8;
        let len = dict.get_int("metadata_size")? as usize;
        Some(Metadata { id, len })
    }

    pub fn data(&self, expected_piece: i64) -> anyhow::Result<&'a [u8]> {
        log::trace!("data: {:#?}", self.value);
        let dict = self.value.as_dict().context("Not a dict")?;

        let msg_type = dict.get_int("msg_type").context("`msg_type` not found")?;
        anyhow::ensure!(msg_type == msg_type::DATA, "Not a DATA message");

        let piece = dict.get_int("piece").context("`piece` not found")?;
        anyhow::ensure!(piece == expected_piece, "Incorrect piece");

        if self.rest.len() > METADATA_PIECE_LEN {
            anyhow::bail!("Piece can't be larger than 16kB");
        }

        Ok(self.rest)
    }
}

#[derive(Debug)]
pub struct Metadata {
    pub id: u8,
    pub len: usize,
}

pub enum MetadataMsg {
    Handshake(u8),
    Request(i64),
    Reject(i64),
    Data(i64, i64),
}

impl Encode for MetadataMsg {
    fn encode(&self, buf: &mut Vec<u8>) {
        let mut dict = DictEncoder::new(buf);
        match *self {
            MetadataMsg::Handshake(id) => {
                let mut m = dict.insert_dict("m");
                m.insert("ut_metadata", i64::from(id));
                m.finish();

                dict.insert("p", 6881);
                dict.insert("reqq", 500);
            }
            MetadataMsg::Request(piece) => {
                dict.insert("msg_type", msg_type::REQUEST);
                dict.insert("piece", piece);
            }
            MetadataMsg::Reject(piece) => {
                dict.insert("msg_type", msg_type::REJECT);
                dict.insert("piece", piece);
            }
            MetadataMsg::Data(piece, total_size) => {
                dict.insert("msg_type", msg_type::DATA);
                dict.insert("piece", piece);
                dict.insert("total_size", total_size);
            }
        }
        dict.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extended_new() {
        let mut parser = Parser::new();
        let ext = ExtendedMessage::parse(&[0, b'd', b'e', 1, 2, 3, 4], &mut parser).unwrap();
        assert_eq!(0, ext.id);
        assert!(ext.value.is_dict());
        assert_eq!(b"de", ext.value.as_raw_bytes());
        assert_eq!(&[1, 2, 3, 4], ext.rest);
    }

    #[test]
    fn extended_new_2() {
        let mut parser = Parser::new();
        let ext = ExtendedMessage::parse(&[0, b'd', b'e'], &mut parser).unwrap();
        assert_eq!(0, ext.id);
        assert!(ext.value.is_dict());
        assert_eq!(b"de", ext.value.as_raw_bytes());
        assert!(ext.rest.is_empty());
    }
}
