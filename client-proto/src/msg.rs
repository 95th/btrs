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

pub fn packet_header_len(id: u8) -> usize {
    match id {
        PIECE => 8,
        HAVE => 4,
        REQUEST | CANCEL => 12,
        _ => 0,
    }
}

#[derive(Debug, PartialEq)]
pub enum Packet {
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have { index: u32 },
    Bitfield { len: u32 },
    Request { index: u32, begin: u32, len: u32 },
    Piece { index: u32, begin: u32, len: u32 },
    Cancel { index: u32, begin: u32, len: u32 },
    Extended { len: u32 },
    Unknown { id: u8, len: u32 },
}

impl Packet {
    pub fn read(len: u32, id: u8, data: &mut impl Buf) -> Packet {
        match id {
            CHOKE => Packet::Choke,
            UNCHOKE => Packet::Unchoke,
            INTERESTED => Packet::Interested,
            NOT_INTERESTED => Packet::NotInterested,
            HAVE => Packet::Have {
                index: data.get_u32(),
            },
            BITFIELD => Packet::Bitfield { len: len - 1 },
            REQUEST => Packet::Request {
                index: data.get_u32(),
                begin: data.get_u32(),
                len: data.get_u32(),
            },
            PIECE => Packet::Piece {
                index: data.get_u32(),
                begin: data.get_u32(),
                len: len - 9,
            },
            CANCEL => Packet::Cancel {
                index: data.get_u32(),
                begin: data.get_u32(),
                len: data.get_u32(),
            },
            EXTENDED => Packet::Extended { len: len - 1 },
            _ => Packet::Unknown { id, len: len - 1 },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::conn::Connection;

    use super::*;

    #[test]
    fn parse_piece() {
        let mut c = Connection::new();
        c.send_piece(1, 2, &[]);
        let mut b = &c.get_send_buf()[..];

        let len = b.get_u32();
        let id = b.get_u8();
        let packet = Packet::read(len, id, &mut b);

        assert_eq!(
            packet,
            Packet::Piece {
                index: 1,
                begin: 2,
                len: 0
            }
        );
    }

    #[test]
    fn parse_choke() {
        let mut c = Connection::new();
        c.send_choke();
        let mut b = &c.get_send_buf()[..];

        let len = b.get_u32();
        let id = b.get_u8();
        let packet = Packet::read(len, id, &mut b);

        assert_eq!(packet, Packet::Choke);
    }

    #[test]
    fn parse_have() {
        let mut c = Connection::new();
        c.send_have(2);
        let mut b = &c.get_send_buf()[..];

        let len = b.get_u32();
        let id = b.get_u8();
        let packet = Packet::read(len, id, &mut b);

        assert_eq!(packet, Packet::Have { index: 2 });
    }

    #[test]
    fn parse_extended() {
        let mut c = Connection::new();
        c.send_extended(0, &[]);
        let mut b = &c.get_send_buf()[..];

        let len = b.get_u32();
        let id = b.get_u8();
        let packet = Packet::read(len, id, &mut b);

        assert_eq!(packet, Packet::Extended { len: 1 });
    }

    #[test]
    fn parse_bitfield() {
        let mut c = Connection::new();
        c.send_bitfield();
        let mut b = &c.get_send_buf()[..];

        let len = b.get_u32();
        let id = b.get_u8();
        let packet = Packet::read(len, id, &mut b);

        assert_eq!(packet, Packet::Bitfield { len: 0 });
    }
}
