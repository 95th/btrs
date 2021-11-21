use anyhow::Context;
use ben::{decode::Dict, Parser};
use thiserror::Error;

pub struct MetaInfo {
    pub name: Option<String>,
    pub length: usize,
    pub piece_len: usize,
    pub pieces: Vec<u8>,
}

impl MetaInfo {
    pub fn parse(data: &[u8]) -> anyhow::Result<Self> {
        Self::parse_with(data, &mut Parser::new())
    }

    pub fn parse_with(data: &[u8], parser: &mut Parser) -> anyhow::Result<Self> {
        use ParseError::*;
        let info = parser.parse::<Dict>(data)?;

        let length = info.get_int("length").context(LengthRequired)?;
        let piece_len = info.get_int("piece length").context(PieceLengthRequired)?;
        let pieces = info.get_bytes("pieces").context(PiecesRequired)?;
        let name = info.get_str("name").map(String::from);

        Ok(MetaInfo {
            name,
            length,
            piece_len,
            pieces: pieces.to_vec(),
        })
    }
}

#[derive(Error, Debug)]
pub(crate) enum ParseError {
    #[error("Torrent Piece hash is required")]
    PiecesRequired,

    #[error("Torrent Piece length is required")]
    PieceLengthRequired,

    #[error("Torrent Info dictionary is required")]
    InfoDictRequired,

    #[error("Torrent length is required")]
    LengthRequired,

    #[error("Announce URL is required")]
    AnnounceRequired,
}
