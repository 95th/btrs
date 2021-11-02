use bytes::{Buf, BytesMut};

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
    Keepalive,
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
    pub fn read(data: &mut BytesMut) -> Result<Self, usize> {
        let mut b = &data[..];

        check_data(&b, 4)?;
        let len = b.get_u32();

        if len == 0 {
            data.advance(4);
            return Ok(Packet::Keepalive);
        }

        check_data(&b, 1)?;
        let id = b.get_u8();

        let packet = match id {
            CHOKE => Packet::Choke,
            UNCHOKE => Packet::Unchoke,
            INTERESTED => Packet::Interested,
            NOT_INTERESTED => Packet::NotInterested,
            HAVE => {
                check_data(&b, 4)?;
                Packet::Have { index: b.get_u32() }
            }
            BITFIELD => Packet::Bitfield { len: len - 1 },
            REQUEST => {
                check_data(&b, 12)?;
                Packet::Request {
                    index: b.get_u32(),
                    begin: b.get_u32(),
                    len: b.get_u32(),
                }
            }
            PIECE => {
                check_data(&b, 8)?;
                Packet::Piece {
                    index: b.get_u32(),
                    begin: b.get_u32(),
                    len: len - 9,
                }
            }
            CANCEL => {
                check_data(&b, 12)?;
                Packet::Cancel {
                    index: b.get_u32(),
                    begin: b.get_u32(),
                    len: b.get_u32(),
                }
            }
            EXTENDED => Packet::Extended { len: len - 1 },
            _ => Packet::Unknown { id, len: len - 1 },
        };

        let read = data.remaining() - b.remaining();
        data.advance(read);

        Ok(packet)
    }
}

fn check_data(b: &impl Buf, len: usize) -> Result<(), usize> {
    if b.remaining() < len {
        return Err(len - b.remaining());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use bytes::BufMut;

    use crate::conn::Connection;

    use super::*;

    #[test]
    fn parse_piece() {
        let mut c = Connection::new();
        c.send_piece(1, 2, &[]);
        let mut b = c.get_send_buf()[..].into();

        let packet = Packet::read(&mut b).unwrap();

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
    fn parse_piece_incomplete() {
        let mut b = BytesMut::new();
        b.put_u32(15);
        b.put_u8(PIECE);
        b.put_u32(2);

        let err = Packet::read(&mut b).unwrap_err();
        assert_eq!(err, 4);
    }

    #[test]
    fn parse_choke() {
        let mut c = Connection::new();
        c.send_choke();
        let mut b = c.get_send_buf()[..].into();
        let packet = Packet::read(&mut b).unwrap();
        assert_eq!(packet, Packet::Choke);
    }

    #[test]
    fn parse_have() {
        let mut c = Connection::new();
        c.send_have(2);
        let mut b = c.get_send_buf()[..].into();
        let packet = Packet::read(&mut b).unwrap();

        assert_eq!(packet, Packet::Have { index: 2 });
    }

    #[test]
    fn parse_extended() {
        let mut c = Connection::new();
        c.send_extended(0, &[]);
        let mut b = c.get_send_buf()[..].into();
        let packet = Packet::read(&mut b).unwrap();

        assert_eq!(packet, Packet::Extended { len: 1 });
    }

    #[test]
    fn parse_bitfield() {
        let mut c = Connection::new();
        c.send_bitfield();
        let mut b = c.get_send_buf()[..].into();
        let packet = Packet::read(&mut b).unwrap();

        assert_eq!(packet, Packet::Bitfield { len: 0 });
    }
}
