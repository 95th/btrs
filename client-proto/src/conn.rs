use std::ops::Deref;

use ben::Encode;
use bytes::{Buf, BufMut};

use crate::bitfield::Bitfield;
use crate::msg::*;

pub struct Connection {
    send_buf: Vec<u8>,
    encode_buf: Vec<u8>,
    bitfield: Bitfield,
    choked: bool,
    interested: bool,
}

impl Connection {
    pub fn new() -> Self {
        Self {
            send_buf: Vec::with_capacity(1024),
            encode_buf: Vec::with_capacity(1024),
            bitfield: Bitfield::new(),
            choked: true,
            interested: false,
        }
    }

    pub fn send_keepalive(&mut self) {
        self.send_buf.put_u32(0);
    }

    pub fn send_choke(&mut self) {
        self.send_buf.put_u32(1);
        self.send_buf.put_u8(CHOKE);
    }

    pub fn send_unchoke(&mut self) {
        self.send_buf.put_u32(1);
        self.send_buf.put_u8(UNCHOKE);
    }

    pub fn send_interested(&mut self) {
        self.send_buf.put_u32(1);
        self.send_buf.put_u8(INTERESTED);
    }

    pub fn send_not_interested(&mut self) {
        self.send_buf.put_u32(1);
        self.send_buf.put_u8(NOT_INTERESTED);
    }

    pub fn send_have(&mut self, index: u32) {
        self.send_buf.put_u32(5);
        self.send_buf.put_u8(HAVE);
        self.send_buf.put_u32(index);
    }

    pub fn send_bitfield(&mut self) {
        let bytes = self.bitfield.as_bytes();
        self.send_buf.put_u32(bytes.len() as u32 + 1);
        self.send_buf.put_u8(BITFIELD);
        self.send_buf.extend(bytes);
    }

    pub fn send_request(&mut self, index: u32, begin: u32, len: u32) {
        self.send_buf.put_u32(13);
        self.send_buf.put_u8(REQUEST);
        self.send_buf.put_u32(index);
        self.send_buf.put_u32(begin);
        self.send_buf.put_u32(len);
    }

    pub fn send_piece(&mut self, index: u32, begin: u32, data: &[u8]) {
        self.send_buf.put_u32(9 + data.len() as u32);
        self.send_buf.put_u8(PIECE);
        self.send_buf.put_u32(index);
        self.send_buf.put_u32(begin);
        self.send_buf.extend(data);
    }

    pub fn send_cancel(&mut self, index: u32, begin: u32, len: u32) {
        self.send_buf.put_u32(13);
        self.send_buf.put_u8(CANCEL);
        self.send_buf.put_u32(index);
        self.send_buf.put_u32(begin);
        self.send_buf.put_u32(len);
    }

    pub fn send_extended<E: Encode>(&mut self, id: u8, payload: E) {
        self.encode_buf.clear();
        payload.encode(&mut self.encode_buf);

        let len = 2 + self.encode_buf.len() as u32;
        self.send_buf.put_u32(len);
        self.send_buf.put_u8(EXTENDED);
        self.send_buf.put_u8(id);
        self.send_buf.extend(&self.encode_buf);
    }

    pub fn get_send_buf(&mut self) -> SendBuf<'_> {
        SendBuf {
            buf: &mut self.send_buf,
        }
    }

    pub fn is_choked(&self) -> bool {
        self.choked
    }

    pub fn read_packet<'a>(&mut self, mut data: &'a [u8]) -> Option<Packet<'a>> {
        let id = data.get_u8();
        match id {
            CHOKE => self.choked = true,
            UNCHOKE => self.choked = false,
            INTERESTED => {
                self.interested = true;
                self.send_unchoke();
            }
            NOT_INTERESTED => {
                self.interested = false;
                self.send_choke();
            }
            HAVE => {
                let index = data.get_u32();
                self.bitfield.set_bit(index as usize);
            }
            BITFIELD => {
                let len = data.len();
                self.bitfield.copy_from_slice(len * 8, data);
            }
            REQUEST => {
                return Some(Packet::Request {
                    index: data.get_u32(),
                    begin: data.get_u32(),
                    len: data.get_u32(),
                })
            }
            PIECE => {
                return Some(Packet::Piece {
                    index: data.get_u32(),
                    begin: data.get_u32(),
                    data,
                })
            }
            CANCEL => {
                return Some(Packet::Cancel {
                    index: data.get_u32(),
                    begin: data.get_u32(),
                    len: data.get_u32(),
                })
            }
            EXTENDED => return Some(Packet::Extended(data)),
            _ => {}
        }
        None
    }
}

pub struct SendBuf<'a> {
    buf: &'a mut Vec<u8>,
}

impl<'a> Deref for SendBuf<'a> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.buf
    }
}

impl<'a> Drop for SendBuf<'a> {
    fn drop(&mut self) {
        self.buf.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_keepalive() {
        let mut conn = Connection::new();
        conn.send_keepalive();
        assert_eq!(conn.send_buf, &[0, 0, 0, 0])
    }

    #[test]
    fn send_choke() {
        let mut conn = Connection::new();
        conn.send_choke();
        assert_eq!(conn.send_buf, &[0, 0, 0, 1, CHOKE])
    }

    #[test]
    fn send_unchoke() {
        let mut conn = Connection::new();
        conn.send_unchoke();
        assert_eq!(conn.send_buf, &[0, 0, 0, 1, UNCHOKE])
    }

    #[test]
    fn send_interested() {
        let mut conn = Connection::new();
        conn.send_interested();
        assert_eq!(conn.send_buf, &[0, 0, 0, 1, INTERESTED])
    }

    #[test]
    fn send_not_interested() {
        let mut conn = Connection::new();
        conn.send_not_interested();
        assert_eq!(conn.send_buf, &[0, 0, 0, 1, NOT_INTERESTED])
    }

    #[test]
    fn send_have() {
        let mut conn = Connection::new();
        conn.send_have(4);
        assert_eq!(conn.send_buf, &[0, 0, 0, 5, HAVE, 0, 0, 0, 4])
    }

    #[test]
    fn send_bitfield_empty() {
        let mut conn = Connection::new();
        conn.send_bitfield();
        assert_eq!(conn.send_buf, &[0, 0, 0, 1, BITFIELD])
    }

    #[test]
    fn send_bitfield() {
        let mut conn = Connection::new();
        conn.bitfield.resize(3);
        conn.bitfield.set_bit(1);
        conn.send_bitfield();
        assert_eq!(conn.send_buf, &[0, 0, 0, 2, BITFIELD, 0b01000000])
    }

    #[test]
    fn send_request() {
        let mut conn = Connection::new();
        conn.send_request(2, 4, 5);
        assert_eq!(
            conn.send_buf,
            &[0, 0, 0, 13, REQUEST, 0, 0, 0, 2, 0, 0, 0, 4, 0, 0, 0, 5]
        )
    }

    #[test]
    fn send_piece() {
        let mut conn = Connection::new();
        conn.send_piece(3, 5, &[1, 2, 3, 4]);
        assert_eq!(
            conn.send_buf,
            &[0, 0, 0, 13, PIECE, 0, 0, 0, 3, 0, 0, 0, 5, 1, 2, 3, 4]
        )
    }

    #[test]
    fn send_cancel() {
        let mut conn = Connection::new();
        conn.send_cancel(2, 4, 5);
        assert_eq!(
            conn.send_buf,
            &[0, 0, 0, 13, CANCEL, 0, 0, 0, 2, 0, 0, 0, 4, 0, 0, 0, 5]
        )
    }

    #[test]
    fn send_extended() {
        let mut conn = Connection::new();
        conn.send_extended(2, "hello");
        assert_eq!(
            conn.send_buf,
            &[0, 0, 0, 9, EXTENDED, 2, b'5', b':', b'h', b'e', b'l', b'l', b'o']
        )
    }

    #[test]
    fn get_send_buf_clears() {
        let mut conn = Connection::new();
        conn.send_keepalive();
        assert!(!conn.send_buf.is_empty());
        drop(conn.get_send_buf());
        assert!(conn.send_buf.is_empty());
    }

    #[test]
    fn parse_choke() {
        let mut tx = Connection::new();
        let mut rx = Connection::new();
        rx.choked = false;
        tx.send_choke();

        let data = &tx.get_send_buf()[4..];
        assert!(rx.read_packet(data).is_none());
        assert!(rx.choked);
    }

    #[test]
    fn parse_unchoke() {
        let mut rx = Connection::new();
        let mut tx = Connection::new();
        tx.send_unchoke();

        let data = &tx.get_send_buf()[4..];
        assert!(rx.read_packet(data).is_none());
        assert!(!rx.choked);
    }

    #[test]
    fn parse_interested() {
        let mut rx = Connection::new();
        let mut tx = Connection::new();
        tx.send_interested();

        let data = &tx.get_send_buf()[4..];
        assert!(rx.read_packet(data).is_none());
        assert!(rx.interested);
        assert_eq!(rx.send_buf, &[0, 0, 0, 1, UNCHOKE]);
    }

    #[test]
    fn parse_not_interested() {
        let mut rx = Connection::new();
        let mut tx = Connection::new();
        rx.interested = true;
        tx.send_not_interested();

        let data = &tx.get_send_buf()[4..];
        assert!(rx.read_packet(data).is_none());
        assert!(!rx.interested);
        assert_eq!(rx.send_buf, &[0, 0, 0, 1, CHOKE]);
    }

    #[test]
    fn parse_have() {
        let mut rx = Connection::new();
        let mut tx = Connection::new();
        rx.bitfield.resize(16);
        tx.send_have(5);

        let data = &tx.get_send_buf()[4..];
        assert!(rx.read_packet(data).is_none());
        assert_eq!(rx.bitfield.get_bit(5), true);
    }

    #[test]
    fn parse_bitfield() {
        let mut rx = Connection::new();
        let mut tx = Connection::new();
        tx.bitfield.resize(16);
        tx.bitfield.set_bit(5);
        tx.send_bitfield();

        let data = &tx.get_send_buf()[4..];
        assert!(rx.read_packet(data).is_none());
        assert_eq!(rx.bitfield.as_bytes(), &[0b0000_0100, 0b0000_0000]);
    }

    #[test]
    fn parse_request() {
        let mut rx = Connection::new();
        let mut tx = Connection::new();
        tx.send_request(2, 3, 4);

        let data = &tx.get_send_buf()[4..];
        assert_eq!(
            Packet::Request {
                index: 2,
                begin: 3,
                len: 4
            },
            rx.read_packet(data).unwrap()
        );
    }

    #[test]
    fn parse_piece() {
        let mut rx = Connection::new();
        let mut tx = Connection::new();
        tx.send_piece(2, 3, b"hello");

        let data = &tx.get_send_buf()[4..];
        assert_eq!(
            Packet::Piece {
                index: 2,
                begin: 3,
                data: b"hello"
            },
            rx.read_packet(data).unwrap()
        );
    }

    #[test]
    fn parse_cancel() {
        let mut rx = Connection::new();
        let mut tx = Connection::new();
        tx.send_cancel(2, 3, 4);

        let data = &tx.get_send_buf()[4..];
        assert_eq!(
            Packet::Cancel {
                index: 2,
                begin: 3,
                len: 4
            },
            rx.read_packet(data).unwrap()
        );
    }

    #[test]
    fn parse_extended() {
        let mut rx = Connection::new();
        let mut tx = Connection::new();
        tx.send_extended(2, "hello");

        let data = &tx.get_send_buf()[4..];
        assert_eq!(
            Packet::Extended(b"\x025:hello"),
            rx.read_packet(data).unwrap()
        );
    }
}
