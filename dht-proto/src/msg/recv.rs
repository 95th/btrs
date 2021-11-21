use crate::id::NodeId;
use crate::msg::TxnId;
use ben::decode::{Dict, List};
use ben::{Decode, Entry};
use std::convert::TryInto;

#[derive(Debug)]
pub struct Query<'a> {
    pub txn_id: TxnId,
    pub id: NodeId,
    pub kind: QueryKind<'a>,
}

#[derive(Debug, PartialEq)]
pub enum QueryKind<'a> {
    Ping,
    FindNode {
        target: NodeId,
    },
    GetPeers {
        info_hash: NodeId,
    },
    AnnouncePeer {
        info_hash: NodeId,
        implied_port: bool,
        port: u16,
        token: &'a [u8],
    },
}

#[derive(Debug)]
pub struct Response<'a> {
    pub txn_id: TxnId,
    pub body: Dict<'a, 'a>,
    pub id: NodeId,
}

#[derive(Debug)]
pub struct ErrorResponse<'a> {
    pub txn_id: TxnId,
    pub list: Option<List<'a, 'a>>,
}

#[derive(Debug)]
pub enum Msg<'a> {
    Query(Query<'a>),
    Response(Response<'a>),
    Error(ErrorResponse<'a>),
}

macro_rules! node_id {
    ($dict: expr, $key: literal) => {{
        let id = $dict.get_bytes($key)?;
        if id.len() == 20 {
            let ptr = id.as_ptr().cast::<NodeId>();
            unsafe { *ptr }
        } else {
            return None;
        }
    }};
}

impl<'a> Decode<'a, 'a> for Msg<'a> {
    fn decode(entry: Entry<'a, 'a>) -> Option<Self> {
        let dict = entry.as_dict()?;
        let msg_type = dict.get_bytes("y")?;
        let txn_id = dict.get_bytes("t")?;
        let txn_id = txn_id.try_into().ok()?;
        let txn_id = TxnId(u16::from_be_bytes(txn_id));

        let msg = match msg_type {
            b"q" => {
                let kind = dict.get_bytes("q")?;
                let args = dict.get_dict("a")?;

                let query_kind = match kind {
                    b"ping" => QueryKind::Ping,
                    b"find_node" => QueryKind::FindNode {
                        target: node_id!(args, "target"),
                    },
                    b"get_peers" => QueryKind::GetPeers {
                        info_hash: node_id!(args, "info_hash"),
                    },
                    b"announce_peer" => {
                        let implied_port = args
                            .get_int("implied_port")
                            .map(|n: i64| n == 1)
                            .unwrap_or(false);
                        QueryKind::AnnouncePeer {
                            info_hash: node_id!(args, "info_hash"),
                            implied_port,
                            port: args.get_int("port")?,
                            token: args.get_bytes("token")?,
                        }
                    }
                    other => {
                        trace!("Unexpected Query type: {:?}", other);
                        return None;
                    }
                };
                Msg::Query(Query {
                    kind: query_kind,
                    id: node_id!(args, "id"),
                    txn_id,
                })
            }
            b"r" => {
                let body = dict.get_dict("r")?;
                Msg::Response(Response {
                    id: node_id!(body, "id"),
                    txn_id,
                    body,
                })
            }
            b"e" => {
                trace!("Error: {:?}", dict);
                let list = dict.get_list("r");
                Msg::Error(ErrorResponse { txn_id, list })
            }
            other => {
                trace!("Unexpected Message type: {:?}", other);
                return None;
            }
        };

        Some(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ben::Parser;

    #[test]
    fn incoming_ping() {
        let expected: &[u8] = b"d1:ad2:id20:\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01e1:q4:ping1:t2:\x00\n1:y1:qe";
        let mut parser = Parser::new();
        let msg = parser.parse::<Msg>(expected).unwrap();

        match msg {
            Msg::Query(query) => {
                assert_eq!(query.id, NodeId::all(1));
                assert_eq!(query.txn_id, TxnId(10));
                assert_eq!(query.kind, QueryKind::Ping);
            }
            _ => {
                panic!("Incorrect msg type");
            }
        }
    }
}
