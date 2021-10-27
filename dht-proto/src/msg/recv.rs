use crate::id::NodeId;
use crate::msg::TxnId;
use ben::decode::{Dict, List};
use ben::{Decode, Decoder};
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
    pub body: Dict<'a>,
    pub id: NodeId,
}

#[derive(Debug)]
pub struct ErrorResponse<'a> {
    pub txn_id: TxnId,
    pub list: Option<List<'a>>,
}

#[derive(Debug)]
pub enum Msg<'a> {
    Query(Query<'a>),
    Response(Response<'a>),
    Error(ErrorResponse<'a>),
}

macro_rules! check {
    ($expr:expr, $err:literal) => {
        match $expr {
            Some(val) => val,
            None => return Err(ben::Error::Other($err)),
        }
    };
}

macro_rules! node_id {
    ($dict: expr, $key: literal) => {{
        let id = match $dict.get_bytes($key) {
            Some(id) => id,
            None => return Err(ben::Error::Other(concat!($key, " is required"))),
        };
        if id.len() == 20 {
            let ptr = id.as_ptr().cast::<NodeId>();
            unsafe { *ptr }
        } else {
            return Err(ben::Error::Other("Node ID must be 20 bytes long"));
        }
    }};
}

impl<'a> Decode<'a> for Msg<'a> {
    fn decode(decoder: Decoder<'a>) -> ben::Result<Self> {
        use ben::Error::Other;

        let dict = check!(decoder.into_dict(), "Not a dict");
        let msg_type = check!(dict.get_bytes("y"), "Message type is required");
        let txn_id = check!(dict.get_bytes("t"), "Transaction ID is required");
        let txn_id = check!(txn_id.try_into().ok(), "Transaction ID must be 2 bytes");
        let txn_id = TxnId(u16::from_be_bytes(txn_id));

        let msg = match msg_type {
            b"q" => {
                let kind = check!(dict.get_bytes("q"), "Query type is required");
                let args = check!(dict.get_dict("a"), "Arguments are required");

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
                            .map(|n| n == 1)
                            .unwrap_or(false);
                        QueryKind::AnnouncePeer {
                            info_hash: node_id!(args, "info_hash"),
                            implied_port,
                            port: check!(args.get_int("port"), "port is required") as u16,
                            token: check!(args.get_bytes("token"), "Token is required"),
                        }
                    }
                    other => {
                        log::trace!("Unexpected Query type: {:?}", other);
                        return Err(Other("Unexpected Query type"));
                    }
                };
                Msg::Query(Query {
                    kind: query_kind,
                    id: node_id!(args, "id"),
                    txn_id,
                })
            }
            b"r" => {
                let body = check!(dict.get_dict("r"), "Response args are required");
                Msg::Response(Response {
                    id: node_id!(body, "id"),
                    txn_id,
                    body,
                })
            }
            b"e" => {
                log::trace!("Error: {:?}", dict);
                let list = dict.get_list("r");
                Msg::Error(ErrorResponse { txn_id, list })
            }
            other => {
                log::trace!("Unexpected Message type: {:?}", other);
                return Err(Other("Unexpected Message type"));
            }
        };

        Ok(msg)
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
