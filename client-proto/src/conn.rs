use std::ops::Deref;

use ben::Encode;
use bytes::{Buf, BufMut, BytesMut};

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

    pub fn send_extended(&mut self, id: u8, payload: &impl Encode) {
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

    pub fn read_packet(&mut self, data: &mut BytesMut) -> Option<Packet> {
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
                self.bitfield.copy_from_slice(len * 8, &data);
                data.clear();
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
                    data: data.split(),
                })
            }
            CANCEL => {
                return Some(Packet::Cancel {
                    index: data.get_u32(),
                    begin: data.get_u32(),
                    len: data.get_u32(),
                })
            }
            EXTENDED => return Some(Packet::Extended { data: data.split() }),
            _ => data.clear(),
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
