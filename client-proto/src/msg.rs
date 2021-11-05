use bytes::Buf;

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
pub enum Packet<'a> {
    Request {
        index: u32,
        begin: u32,
        len: u32,
    },
    Piece {
        index: u32,
        begin: u32,
        data: &'a [u8],
    },
    Cancel {
        index: u32,
        begin: u32,
        len: u32,
    },
    Extended {
        data: &'a [u8],
    },
}

impl Packet<'_> {
    pub fn header_size(id: u8) -> usize {
        match id {
            HAVE => 4,
            REQUEST | CANCEL => 12,
            PIECE => 8,
            _ => 0,
        }
    }

    pub fn read(mut data: &[u8]) -> Packet {
        let id = data.get_u8();
        match id {
            REQUEST => Packet::Request {
                index: data.get_u32(),
                begin: data.get_u32(),
                len: data.get_u32(),
            },
            PIECE => Packet::Piece {
                index: data.get_u32(),
                begin: data.get_u32(),
                data,
            },
            CANCEL => Packet::Cancel {
                index: data.get_u32(),
                begin: data.get_u32(),
                len: data.get_u32(),
            },
            EXTENDED => Packet::Extended { data },
            _ => panic!("Invalid Packet ID"),
        }
    }
}
