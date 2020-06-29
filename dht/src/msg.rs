use crate::id::NodeId;
use ben::decode::Dict;
use ben::{Decode, Decoder, Encode, Encoder};
use std::convert::TryInto;

#[derive(Debug, Copy, Clone, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct TxnId(pub u16);

impl TxnId {
    pub fn next(&mut self) -> Self {
        let out = *self;
        self.0 = self.0.wrapping_add(1);
        out
    }
}

impl Encode for TxnId {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        enc.add_bytes(&self.0.to_be_bytes()[..]);
    }
}

#[derive(Debug, Clone, Copy, PartialOrd, PartialEq)]
pub enum MsgKind {
    Ping,
    FindNode,
    GetPeers,
    AnnouncePeer,
    Response,
    Error,
}

#[derive(Debug)]
pub struct Msg<'a, 'p> {
    pub kind: MsgKind,
    pub txn_id: TxnId,
    pub id: Option<&'a NodeId>,
    pub body: Decoder<'a, 'p>,
}

macro_rules! check {
    ($expr:expr, $err:literal) => {
        match $expr {
            Some(val) => val,
            None => return Err(ben::Error::Other($err)),
        }
    };
}

impl<'a, 'p> Decode<'a, 'p> for Msg<'a, 'p> {
    fn decode(decoder: Decoder<'a, 'p>) -> ben::Result<Self> {
        use ben::Error::Other;

        let dict = check!(decoder.as_dict(), "Not a dict");
        let y = check!(dict.get_bytes(b"y"), "Message type is required");

        let kind = match y {
            b"q" => {
                let q = check!(dict.get_bytes(b"q"), "Query type is required");
                match q {
                    b"ping" => MsgKind::Ping,
                    b"find_node" => MsgKind::FindNode,
                    b"get_peers" => MsgKind::GetPeers,
                    b"announce_peer" => MsgKind::AnnouncePeer,
                    other => {
                        trace!("Unexpected Query type: {:?}", other);
                        return Err(Other("Unexpected Query type"));
                    }
                }
            }
            b"r" => MsgKind::Response,
            b"e" => MsgKind::Error,
            other => {
                trace!("Unexpected Message type: {:?}", other);
                return Err(Other("Unexpected Message type"));
            }
        };
        let txn_id = check!(dict.get_bytes(b"t"), "Transaction ID is required");
        let txn_id = check!(txn_id.try_into().ok(), "Transaction ID must be 2 bytes");
        let id = get_id(kind, dict);

        Ok(Self {
            kind,
            txn_id: TxnId(u16::from_be_bytes(txn_id)),
            id,
            body: decoder,
        })
    }
}

fn get_id<'a>(kind: MsgKind, dict: &Dict<'a, '_>) -> Option<&'a NodeId> {
    use MsgKind::*;
    let inner = match kind {
        Response => dict.get_dict(b"r")?,
        Ping | FindNode | GetPeers | AnnouncePeer => dict.get_dict(b"a")?,
        Error => return None,
    };
    let id = inner.get_bytes(b"id")?;
    if id.len() == 20 {
        let ptr = id.as_ptr() as *const NodeId;
        unsafe { Some(&*ptr) }
    } else {
        debug!("Unexpected ID length: {}", id.len());
        None
    }
}

pub struct Ping<'a> {
    pub txn_id: TxnId,
    pub id: &'a NodeId,
}

impl Encode for Ping<'_> {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        let mut d = enc.add_dict();

        let mut a = d.add_dict("a");
        a.add("id", self.id);
        a.finish();

        d.add("q", "ping");
        d.add("t", self.txn_id);
        d.add("y", "q");
    }
}

pub struct FindNode<'a> {
    pub txn_id: TxnId,
    pub id: &'a NodeId,
    pub target: &'a NodeId,
}

impl Encode for FindNode<'_> {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        let mut d = enc.add_dict();

        let mut a = d.add_dict("a");
        a.add("id", self.id);
        a.add("target", self.target);
        a.finish();

        d.add("q", "find_node");
        d.add("t", self.txn_id);
        d.add("y", "q");
    }
}

pub struct GetPeers<'a> {
    pub txn_id: TxnId,
    pub id: &'a NodeId,
    pub info_hash: &'a NodeId,
}

impl Encode for GetPeers<'_> {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        let mut d = enc.add_dict();

        let mut a = d.add_dict("a");
        a.add("id", self.id);
        a.add("info_hash", self.info_hash);
        a.finish();

        d.add("q", "get_peers");
        d.add("t", self.txn_id);
        d.add("y", "q");
    }
}

pub struct AnnouncePeer<'a> {
    pub txn_id: TxnId,
    pub id: &'a NodeId,
    pub implied_port: bool,
    pub info_hash: &'a NodeId,
    pub port: u16,
    pub token: &'a [u8],
}

impl Encode for AnnouncePeer<'_> {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        let mut d = enc.add_dict();

        let mut a = d.add_dict("a");
        a.add("id", self.id);
        if self.implied_port {
            a.add("implied_port", 1);
        }

        a.add("info_hash", self.info_hash);

        if !self.implied_port {
            a.add("port", self.port as i64);
        }

        a.add("token", self.token);
        a.finish();

        d.add("q", "announce_peer");
        d.add("t", self.txn_id);
        d.add("y", "q");
    }
}

pub struct Error {
    pub kind: ErrorKind,
    pub description: String,
}

pub enum ErrorKind {
    Generic,
    Server,
    Protocol,
    MethodUnknown,
}

impl Encode for Error {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        use ErrorKind::*;
        let code = match self.kind {
            Generic => 201,
            Server => 202,
            Protocol => 203,
            MethodUnknown => 204,
        };
        enc.add_int(code);
        enc.add_str(&self.description);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ben::Parser;

    #[test]
    fn request_ping() {
        let request = Ping {
            txn_id: TxnId(10),
            id: &NodeId::of_byte(1),
        };

        let encoded = request.encode_to_vec();
        let expected = b"d1:ad2:id20:\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01e1:q4:ping1:t2:\x00\n1:y1:qe";
        assert_eq!(
            encoded[..],
            expected[..],
            "\nExpected : {}\nActual   : {}",
            ascii_escape(expected),
            ascii_escape(&encoded)
        );
    }

    #[test]
    fn request_find_node() {
        let request = FindNode {
            txn_id: TxnId(10),
            id: &NodeId::of_byte(1),
            target: &NodeId::of_byte(2),
        };

        let encoded = request.encode_to_vec();
        let expected = b"d1:ad2:id20:\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x016:target20:\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02e1:q9:find_node1:t2:\x00\n1:y1:qe";
        assert_eq!(
            encoded[..],
            expected[..],
            "\nExpected : {}\nActual   : {}",
            ascii_escape(expected),
            ascii_escape(&encoded)
        );
    }

    #[test]
    fn request_get_peers() {
        let request = GetPeers {
            txn_id: TxnId(10),
            id: &NodeId::of_byte(1),
            info_hash: &NodeId::of_byte(2),
        };

        let encoded = request.encode_to_vec();
        let expected = b"d1:ad2:id20:\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x019:info_hash20:\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02e1:q9:get_peers1:t2:\x00\n1:y1:qe";
        assert_eq!(
            encoded[..],
            expected[..],
            "\nExpected : {}\nActual   : {}",
            ascii_escape(expected),
            ascii_escape(&encoded)
        );
    }

    #[test]
    fn request_announce_peer() {
        let request = AnnouncePeer {
            txn_id: TxnId(10),
            id: &NodeId::of_byte(1),
            info_hash: &NodeId::of_byte(2),
            implied_port: false,
            port: 5000,
            token: &[0, 1, 2],
        };

        let encoded = request.encode_to_vec();
        let expected = b"d1:ad2:id20:\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x019:info_hash20:\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x024:porti5000e5:token3:\x00\x01\x02e1:q13:announce_peer1:t2:\x00\n1:y1:qe";
        assert_eq!(
            encoded[..],
            expected[..],
            "\nExpected : {}\nActual   : {}",
            ascii_escape(expected),
            ascii_escape(&encoded)
        );
    }

    #[test]
    fn request_announce_peer_implied_port() {
        let request = AnnouncePeer {
            txn_id: TxnId(10),
            id: &NodeId::of_byte(1),
            info_hash: &NodeId::of_byte(2),
            implied_port: true,
            port: 5000,
            token: &[0, 1, 2],
        };

        let encoded = request.encode_to_vec();
        let expected = b"d1:ad2:id20:\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x0112:implied_porti1e9:info_hash20:\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x025:token3:\x00\x01\x02e1:q13:announce_peer1:t2:\x00\n1:y1:qe";
        assert_eq!(
            encoded[..],
            expected[..],
            "\nExpected : {}\nActual   : {}",
            ascii_escape(expected),
            ascii_escape(&encoded)
        );
    }

    #[test]
    fn incoming_ping() {
        let expected: &[u8] = b"d1:ad2:id20:\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01e1:q4:ping1:t2:\x00\n1:y1:qe";
        let mut parser = Parser::new();
        let msg = parser.parse::<Msg>(expected).unwrap();
        assert!(matches!(msg.kind, MsgKind::Ping));
    }

    fn ascii_escape(buf: &[u8]) -> String {
        use std::ascii::escape_default;
        let v = buf.iter().flat_map(|&c| escape_default(c)).collect();
        // Safety: output of escape_default is valid UTF-8
        unsafe { String::from_utf8_unchecked(v) }
    }
}
