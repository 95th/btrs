use std::collections::VecDeque;
use std::ops::Deref;

use anyhow::Context;
use ben::{Encode, Parser};
use bytes::{Buf, BufMut};

use crate::bitfield::Bitfield;
use crate::event::Event;
use crate::ext::{ExtendedMessage, MetadataMsg};
use crate::handshake::Handshake;
use crate::state::{Error, MetadataState, State};
use crate::{msg::*, InfoHash, PeerId};

pub struct Connection {
    send_buf: Vec<u8>,
    encode_buf: Vec<u8>,
    bitfield: Bitfield,
    choked: bool,
    interested: bool,
    state: State,
    parser: Parser,
    events: VecDeque<Event>,
}

macro_rules! ensure_state {
    ($self:expr, $( $state:pat_param )|+ ) => {
        ensure!(matches!($self.state, $( $state )|+), Error::InvalidState)
    };
}

impl Connection {
    pub fn new() -> Self {
        Self {
            send_buf: Vec::with_capacity(1024),
            encode_buf: Vec::with_capacity(1024),
            bitfield: Bitfield::new(),
            choked: true,
            interested: false,
            state: State::HandshakeRequired,
            parser: Parser::new(),
            events: VecDeque::new(),
        }
    }

    pub fn poll_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    pub fn send_handshake(&mut self, info_hash: &InfoHash, peer_id: &PeerId) -> anyhow::Result<()> {
        ensure_state!(self, State::HandshakeRequired);
        let mut h = Handshake::new(*info_hash, *peer_id);
        h.set_extended(true);
        self.send_buf.extend_from_slice(h.as_bytes());
        self.state = State::HandshakeSent;
        Ok(())
    }

    pub fn recv_handshake(
        &mut self,
        info_hash: &InfoHash,
        data: [u8; 68],
    ) -> anyhow::Result<PeerId> {
        ensure_state!(self, State::HandshakeSent);
        let h: Handshake = unsafe { std::mem::transmute(data) };
        ensure!(h.is_supported(), Error::UnsupportedProtocol);
        ensure!(h.info_hash == *info_hash, Error::UnsupportedProtocol);
        self.state = State::Ready;
        Ok(h.peer_id)
    }

    pub fn recv_metadata(&mut self, data: &[u8]) -> anyhow::Result<()> {
        use State::*;

        match std::mem::replace(&mut self.state, State::Ready) {
            Ready => {
                if let Some(Packet::Extended(ext)) = self.recv_packet(data) {
                    let ext = ExtendedMessage::parse(ext, &mut self.parser)?;
                    ensure!(ext.is_handshake(), Error::InvalidHandshake);

                    let meta = ext.metadata().context(Error::MetadataUnsupported)?;
                    ensure!(meta.len < 50 * 1024 * 1024, Error::PacketTooLarge);

                    self.send_ext(meta.id, MetadataMsg::Handshake(meta.id, 0));
                    self.send_ext(meta.id, MetadataMsg::Request(0));

                    self.state = MetadataRequested(MetadataState {
                        ext_id: meta.id,
                        requested_piece: 0,
                        len: meta.len,
                        buf: Vec::new(),
                    });
                }
            }
            MetadataRequested(mut state) => {
                if let Some(Packet::Extended(ext)) = self.recv_packet(data) {
                    let ext = ExtendedMessage::parse(ext, &mut self.parser)?;
                    let piece = ext.data(state.requested_piece)?;
                    trace!("Got piece response, len: {}", piece.len());
                    state.buf.extend_from_slice(piece);

                    ensure!(state.buf.len() <= state.len, Error::PacketTooLarge);

                    if state.buf.len() == state.len {
                        self.events.push_back(Event::Metadata(state.buf));
                        self.state = State::Ready;
                        return Ok(());
                    }

                    state.requested_piece += 1;
                    self.send_ext(state.ext_id, MetadataMsg::Request(state.requested_piece));
                }

                self.state = MetadataRequested(state);
            }
            _ => bail!(Error::InvalidState),
        }

        Ok(())
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
        self.send_buf.extend_from_slice(bytes);
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
        self.send_buf.extend_from_slice(data);
    }

    pub fn send_cancel(&mut self, index: u32, begin: u32, len: u32) {
        self.send_buf.put_u32(13);
        self.send_buf.put_u8(CANCEL);
        self.send_buf.put_u32(index);
        self.send_buf.put_u32(begin);
        self.send_buf.put_u32(len);
    }

    pub fn send_ext<E: Encode>(&mut self, id: u8, payload: E) {
        self.encode_buf.clear();
        payload.encode(&mut self.encode_buf);

        let len = 2 + self.encode_buf.len() as u32;
        self.send_buf.put_u32(len);
        self.send_buf.put_u8(EXTENDED);
        self.send_buf.put_u8(id);
        self.send_buf.extend_from_slice(&self.encode_buf);
    }

    pub fn send_ext_data<E: Encode>(&mut self, id: u8, payload: E, data: &[u8]) {
        self.encode_buf.clear();
        payload.encode(&mut self.encode_buf);

        let len = 2 + self.encode_buf.len() + data.len();
        self.send_buf.put_u32(len as u32);
        self.send_buf.put_u8(EXTENDED);
        self.send_buf.put_u8(id);
        self.send_buf.extend_from_slice(&self.encode_buf);
        self.send_buf.extend_from_slice(data);
    }

    pub fn get_send_buf(&mut self) -> SendBuf<'_> {
        SendBuf {
            buf: &mut self.send_buf,
        }
    }

    pub fn is_choked(&self) -> bool {
        self.choked
    }

    pub fn recv_packet<'a>(&mut self, mut data: &'a [u8]) -> Option<Packet<'a>> {
        let id = data.get_u8();
        match id {
            CHOKE => {
                trace!("Got choke");
                self.choked = true;
            }
            UNCHOKE => {
                trace!("Got unchoke");
                self.choked = false;
            }
            INTERESTED => {
                trace!("Got interested");
                self.interested = true;
                self.send_unchoke();
            }
            NOT_INTERESTED => {
                trace!("Got not-interested");
                self.interested = false;
                self.send_choke();
            }
            HAVE => {
                let index = data.get_u32();
                trace!("Got have: {}", index);
                self.bitfield.set_bit(index as usize);
            }
            BITFIELD => {
                let len = data.len();
                trace!("Got bitfield len: {}", len);
                self.bitfield.copy_from_slice(len * 8, data);
            }
            REQUEST => {
                let index = data.get_u32();
                let begin = data.get_u32();
                let len = data.get_u32();
                trace!("Got Request: index {}, begin {}, len {}", index, begin, len);
                return Some(Packet::Request { index, begin, len });
            }
            PIECE => {
                let index = data.get_u32();
                let begin = data.get_u32();
                trace!("Got Piece: index {}, begin {}", index, begin);
                return Some(Packet::Piece(PieceBlock { index, begin, data }));
            }
            CANCEL => {
                let index = data.get_u32();
                let begin = data.get_u32();
                let len = data.get_u32();
                trace!("Got Request: index {}, begin {}, len {}", index, begin, len);
                return Some(Packet::Cancel { index, begin, len });
            }
            EXTENDED => {
                trace!("Got Extended: len {}", data.len());
                return Some(Packet::Extended(data));
            }
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
        conn.send_ext(2, "hello");
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
        assert!(rx.recv_packet(data).is_none());
        assert!(rx.choked);
    }

    #[test]
    fn parse_unchoke() {
        let mut rx = Connection::new();
        let mut tx = Connection::new();
        tx.send_unchoke();

        let data = &tx.get_send_buf()[4..];
        assert!(rx.recv_packet(data).is_none());
        assert!(!rx.choked);
    }

    #[test]
    fn parse_interested() {
        let mut rx = Connection::new();
        let mut tx = Connection::new();
        tx.send_interested();

        let data = &tx.get_send_buf()[4..];
        assert!(rx.recv_packet(data).is_none());
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
        assert!(rx.recv_packet(data).is_none());
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
        assert!(rx.recv_packet(data).is_none());
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
        assert!(rx.recv_packet(data).is_none());
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
            rx.recv_packet(data).unwrap()
        );
    }

    #[test]
    fn parse_piece() {
        let mut rx = Connection::new();
        let mut tx = Connection::new();
        tx.send_piece(2, 3, b"hello");

        let data = &tx.get_send_buf()[4..];
        assert_eq!(
            Packet::Piece(PieceBlock {
                index: 2,
                begin: 3,
                data: b"hello"
            }),
            rx.recv_packet(data).unwrap()
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
            rx.recv_packet(data).unwrap()
        );
    }

    #[test]
    fn parse_extended() {
        let mut rx = Connection::new();
        let mut tx = Connection::new();
        tx.send_ext(2, "hello");

        let data = &tx.get_send_buf()[4..];
        assert_eq!(
            Packet::Extended(b"\x025:hello"),
            rx.recv_packet(data).unwrap()
        );
    }

    #[test]
    fn handshake() {
        let mut c = Connection::new();
        assert_eq!(c.state, State::HandshakeRequired);

        c.send_handshake(&[0; 20], &[1; 20]).unwrap();
        assert_eq!(c.state, State::HandshakeSent);

        let h = Handshake::new([0; 20], [2; 20]);
        let p = c.recv_handshake(&[0; 20], *h.as_bytes()).unwrap();
        assert_eq!(p, [2; 20]);
        assert_eq!(c.state, State::Ready);
    }

    #[test]
    fn get_metadata() {
        let mut c = Connection::new();
        let mut sender = Connection::new();

        // Assume handshake is done
        c.state = State::Ready;

        sender.send_ext(0, MetadataMsg::Handshake(2, 20));
        c.recv_metadata(&sender.get_send_buf()[4..]).unwrap();

        assert_eq!(
            c.state,
            State::MetadataRequested(MetadataState {
                ext_id: 2,
                len: 20,
                requested_piece: 0,
                buf: vec![]
            })
        );

        assert_eq!(c.poll_event(), None);

        sender.send_ext_data(1, MetadataMsg::Data(0, 10), b"xxxxxyyyyy");
        c.recv_metadata(&sender.get_send_buf()[4..]).unwrap();

        assert_eq!(
            c.state,
            State::MetadataRequested(MetadataState {
                ext_id: 2,
                len: 20,
                requested_piece: 1,
                buf: b"xxxxxyyyyy".to_vec()
            })
        );

        assert_eq!(c.poll_event(), None);

        sender.send_ext_data(1, MetadataMsg::Data(1, 10), b"tttttqqqqq");
        c.recv_metadata(&sender.get_send_buf()[4..]).unwrap();

        assert_eq!(c.state, State::Ready);

        assert_eq!(
            c.poll_event().unwrap(),
            Event::Metadata(b"xxxxxyyyyytttttqqqqq".to_vec())
        );
    }

    #[test]
    fn get_metadata_without_handshake() {
        let mut c = Connection::new();
        let e = c.recv_metadata(&[]).unwrap_err();
        assert_eq!(e.to_string(), Error::InvalidState.to_string());
    }

    #[test]
    fn get_metadata_with_other_interleaving_msg() {
        let mut c = Connection::new();
        let mut sender = Connection::new();

        // Assume handshake is done
        c.state = State::Ready;

        sender.send_ext(0, MetadataMsg::Handshake(2, 10));
        c.recv_metadata(&sender.get_send_buf()[4..]).unwrap();

        assert_eq!(
            c.state,
            State::MetadataRequested(MetadataState {
                ext_id: 2,
                len: 10,
                requested_piece: 0,
                buf: vec![]
            })
        );

        assert_eq!(c.poll_event(), None);

        // A wild choke appears
        sender.send_choke();
        c.recv_metadata(&sender.get_send_buf()[4..]).unwrap();

        assert_eq!(
            c.state,
            State::MetadataRequested(MetadataState {
                ext_id: 2,
                len: 10,
                requested_piece: 0,
                buf: vec![]
            })
        );
        assert_eq!(c.poll_event(), None);

        sender.send_ext_data(1, MetadataMsg::Data(0, 10), b"xxxxxyyyyy");
        c.recv_metadata(&sender.get_send_buf()[4..]).unwrap();
        assert_eq!(c.state, State::Ready);

        assert_eq!(
            c.poll_event().unwrap(),
            Event::Metadata(b"xxxxxyyyyy".to_vec())
        );
    }
}
