use bytes::BytesMut;

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

#[derive(Debug)]
pub enum Packet {
    Request {
        index: u32,
        begin: u32,
        len: u32,
    },
    Piece {
        index: u32,
        begin: u32,
        data: BytesMut,
    },
    Cancel {
        index: u32,
        begin: u32,
        len: u32,
    },
    Extended {
        data: BytesMut,
    },
}
