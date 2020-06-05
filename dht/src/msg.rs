use crate::id::NodeId;
use anyhow::{bail, Context};
use ben::{Encode, Encoder, Node as BencodeNode};
use std::convert::TryInto;

#[derive(Copy, Clone)]
pub struct TxnId(u16);

impl TxnId {
    pub fn new(n: u16) -> Self {
        Self(n)
    }
}

impl Encode for TxnId {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        enc.add_bytes(&self.0.to_be_bytes()[..]);
    }
}

pub enum MsgKind {
    Query(QueryKind),
    Response,
    Error,
}

pub enum QueryKind {
    Ping,
    FindNode,
    GetPeers,
    AnnouncePeer,
}

pub struct IncomingMsg<'a> {
    pub txn_id: TxnId,
    pub kind: MsgKind,
    pub body: BencodeNode<'a>,
}

impl<'a> IncomingMsg<'a> {
    pub fn parse(buf: &'a [u8]) -> anyhow::Result<Self> {
        let node = BencodeNode::parse(buf)?;
        let dict = node.as_dict().context("Message must be a dict")?;

        let y = dict.get_str(b"y").context("Message type is required")?;
        let kind = match y {
            "q" => {
                let q = dict.get_str(b"q").context("Query type is required")?;
                let query_kind = match q {
                    "ping" => QueryKind::Ping,
                    "find_node" => QueryKind::FindNode,
                    "get_peers" => QueryKind::GetPeers,
                    "announce_peer" => QueryKind::AnnouncePeer,
                    other => bail!("Unexpected query type: {}", other),
                };
                MsgKind::Query(query_kind)
            }
            "r" => MsgKind::Response,
            "e" => MsgKind::Error,
            other => bail!("Unexpected message type: {}", other),
        };
        let txn_id = dict.get(b"t").context("Transaction ID is required")?.data();
        let txn_id = txn_id.try_into()?;
        Ok(Self {
            txn_id: TxnId(u16::from_be_bytes(txn_id)),
            kind,
            body: node,
        })
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
        a.add("info_hash", self.info_hash);
        if self.implied_port {
            a.add("implied_port", 1);
        } else {
            a.add("port", self.port as i64);
        }
        a.add("token", self.token);
        a.finish();

        d.add("q", "announce_peer");
        d.add("t", self.txn_id);
        d.add("y", "q");
    }
}

pub enum Query {
    Ping {
        id: Box<NodeId>,
    },
    FindNode {
        id: Box<NodeId>,
        target: Box<NodeId>,
    },
    GetPeers {
        id: Box<NodeId>,
        info_hash: Box<NodeId>,
    },
    AnnouncePeer {
        id: Box<NodeId>,
        implied_port: bool,
        info_hash: Box<NodeId>,
        port: u16,
        token: Vec<u8>,
    },
}

pub struct Error {
    code: i64,
    description: String,
}

impl Encode for Error {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        enc.add_int(self.code);
        enc.add_str(&self.description);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let expected = b"d1:ad2:id20:\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x019:info_hash20:\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x0212:implied_porti1e5:token3:\x00\x01\x02e1:q13:announce_peer1:t2:\x00\n1:y1:qe";
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
        let msg = IncomingMsg::parse(expected).unwrap();
        assert!(matches!(msg.kind, MsgKind::Query(QueryKind::Ping)))
    }

    fn ascii_escape(buf: &[u8]) -> String {
        use std::ascii::escape_default;
        let v = buf.iter().flat_map(|&c| escape_default(c)).collect();
        // Safety: output of escape_default is valid UTF-8
        unsafe { String::from_utf8_unchecked(v) }
    }
}
