use std::ops::Deref;

use bytes::BufMut;

use crate::bitfield::Bitfield;
use crate::msg::*;

pub struct Connection {
    send_buf: Vec<u8>,
    bitfield: Bitfield,
    choked: bool,
    interested: bool,
}

impl Connection {
    pub fn new() -> Self {
        Self {
            send_buf: Vec::with_capacity(1024),
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

    pub fn set_choked(&mut self, choked: bool) {
        self.choked = choked;
    }

    pub fn set_interested(&mut self, interested: bool) {
        self.interested = interested;
        if interested {
            self.send_unchoke();
        } else {
            self.send_choke();
        }
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

    pub fn send_piece(&mut self, index: u32, begin: u32, len: u32) {
        self.send_buf.put_u32(9 + len);
        self.send_buf.put_u8(PIECE);
        self.send_buf.put_u32(index);
        self.send_buf.put_u32(begin);
    }

    pub fn send_cancel(&mut self, index: u32, begin: u32, len: u32) {
        self.send_buf.put_u32(13);
        self.send_buf.put_u8(CANCEL);
        self.send_buf.put_u32(index);
        self.send_buf.put_u32(begin);
        self.send_buf.put_u32(len);
    }

    pub fn send_extended(&mut self, id: u8, payload: &[u8]) {
        let len = 2 + payload.len() as u32;
        self.send_buf.put_u32(len);
        self.send_buf.put_u8(EXTENDED);
        self.send_buf.put_u8(id);
        self.send_buf.extend(payload);
    }

    pub fn get_send_buf(&mut self) -> SendBuf<'_> {
        SendBuf {
            buf: &mut self.send_buf,
        }
    }

    pub fn recv_have(&mut self, index: u32) {
        self.bitfield.set_bit(index as usize);
    }

    pub fn recv_choke(&mut self) {
        self.choked = true;
    }

    pub fn recv_unchoke(&mut self) {
        self.choked = false;
    }

    pub fn recv_interested(&mut self) {
        self.interested = true;
        self.send_unchoke();
    }

    pub fn recv_not_interested(&mut self) {
        self.interested = false;
        self.send_choke();
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
