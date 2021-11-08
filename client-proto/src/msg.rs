pub const CHOKE: u8 = 0;
pub const UNCHOKE: u8 = 1;
pub const INTERESTED: u8 = 2;
pub const NOT_INTERESTED: u8 = 3;
pub const HAVE: u8 = 4;
pub const BITFIELD: u8 = 5;
pub const REQUEST: u8 = 6;
pub const PIECE: u8 = 7;
pub const CANCEL: u8 = 8;
pub const EXTENDED: u8 = 20;

#[derive(Debug, PartialEq)]
pub enum Packet<'a> {
    Request { index: u32, begin: u32, len: u32 },
    Piece(PieceBlock<'a>),
    Cancel { index: u32, begin: u32, len: u32 },
    Extended(&'a [u8]),
}

impl Packet<'_> {
    pub fn header_len(id: u8) -> usize {
        match id {
            HAVE => 4,
            REQUEST | CANCEL => 12,
            PIECE => 8,
            _ => 0,
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct PieceBlock<'a> {
    pub index: u32,
    pub begin: u32,
    pub data: &'a [u8],
}
