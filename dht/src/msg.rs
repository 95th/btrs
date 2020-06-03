use crate::id::NodeId;
use anyhow::Context;
use ben::{Encode, Encoder, Node};
use std::convert::TryInto;

pub struct TxnId(pub u16);

impl Encode for TxnId {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        enc.add_bytes(&self.0.to_be_bytes()[..]);
    }
}

pub struct Request {
    pub txn_id: TxnId,
    pub query: Query,
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

impl Encode for Request {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        let mut dict = enc.add_dict();
        dict.add("a", &self.query);
        dict.add(
            "q",
            match self.query {
                Query::Ping { .. } => "ping",
                Query::FindNode { .. } => "find_node",
                Query::GetPeers { .. } => "get_peers",
                Query::AnnouncePeer { .. } => "announce_peer",
            },
        );
        dict.add("t", &self.txn_id);
        dict.add("y", "q");
        dict.finish();
    }
}

impl Encode for Query {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        let mut dict = enc.add_dict();
        match self {
            Query::Ping { id } => {
                dict.add("id", id);
            }
            Query::FindNode { id, target } => {
                dict.add("id", id);
                dict.add("target", target);
            }
            Query::GetPeers { id, info_hash } => {
                dict.add("id", id);
                dict.add("info_hash", info_hash);
            }
            Query::AnnouncePeer {
                id,
                implied_port,
                info_hash,
                port,
                token,
            } => {
                dict.add("id", id);
                dict.add("info_hash", info_hash);
                if *implied_port {
                    dict.add("implied_port", 1);
                } else {
                    dict.add("port", *port as i64);
                }
                dict.add("token", &token[..]);
            }
        }
        dict.finish();
    }
}

pub struct Response<'a> {
    txn_id: TxnId,
    kind: ResponseKind,
    data: ben::Node<'a>,
}

pub enum ResponseKind {
    Query,
    Response,
    Error,
}

impl<'a> Response<'a> {
    pub fn parse(buf: &'a [u8]) -> anyhow::Result<Response<'a>> {
        let node = Node::parse(buf)?;
        let dict = node.as_dict().context("Response must be a dictionary")?;

        let resp_type = dict.get_str(b"y").context("Response type not found")?;
        let txn_id = dict
            .get(b"t")
            .context("Transaction ID not found")?
            .data()
            .try_into()
            .context("Transaction ID must be 2 bytes long")?;
        let txn_id = TxnId(u16::from_be_bytes(txn_id));

        let kind = match resp_type {
            "q" => {
                dict.get_dict(b"a").context("Args data not found")?;
                ResponseKind::Query
            }
            "r" => {
                dict.get_dict(b"r").context("Response data not found")?;
                ResponseKind::Response
            }
            "e" => {
                dict.get_list(b"e").context("Error data not found")?;
                ResponseKind::Error
            }
            _ => bail!("Unexpected response type: {}", resp_type),
        };

        Ok(Response {
            txn_id,
            kind,
            data: node,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_ping() {
        let request = Request {
            txn_id: TxnId(10),
            query: Query::Ping {
                id: Box::new(NodeId::of_byte(1)),
            },
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
        let request = Request {
            txn_id: TxnId(10),
            query: Query::FindNode {
                id: Box::new(NodeId::of_byte(1)),
                target: Box::new(NodeId::of_byte(2)),
            },
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
        let request = Request {
            txn_id: TxnId(10),
            query: Query::GetPeers {
                id: Box::new(NodeId::of_byte(1)),
                info_hash: Box::new(NodeId::of_byte(2)),
            },
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
        let request = Request {
            txn_id: TxnId(10),
            query: Query::AnnouncePeer {
                id: Box::new(NodeId::of_byte(1)),
                info_hash: Box::new(NodeId::of_byte(2)),
                implied_port: false,
                port: 5000,
                token: vec![0, 1, 2],
            },
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
        let request = Request {
            txn_id: TxnId(10),
            query: Query::AnnouncePeer {
                id: Box::new(NodeId::of_byte(1)),
                info_hash: Box::new(NodeId::of_byte(2)),
                implied_port: true,
                port: 5000,
                token: vec![0, 1, 2],
            },
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

    fn ascii_escape(buf: &[u8]) -> String {
        use std::ascii::escape_default;
        let v = buf.iter().flat_map(|&c| escape_default(c)).collect();
        // Safety: output of escape_default is valid UTF-8
        unsafe { String::from_utf8_unchecked(v) }
    }
}
