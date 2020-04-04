use crate::id::NodeId;
use ben::{Encode, Encoder};
use std::net::SocketAddr;

type TxnId = u16;

enum Query {
    Ping,
    FindNode,
    GetPeers,
    AnnouncePeer,
}

impl Encode for Query {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        match self {
            Self::Ping => "ping".encode(enc),
            Self::FindNode => "find_node".encode(enc),
            Self::GetPeers => "get_peers".encode(enc),
            Self::AnnouncePeer => "announce_peer".encode(enc),
        }
    }
}

enum MsgKind {
    Query,
    Response,
    Error,
}

impl Encode for MsgKind {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        match self {
            Self::Query => "q".encode(enc),
            Self::Response => "r".encode(enc),
            Self::Error => "e".encode(enc),
        }
    }
}

pub struct Msg {
    query: Query,
    args: MsgArgs,
    txn_id: TxnId,
    kind: MsgKind,
    response: Response,
    error: Error,
    ip: SocketAddr,
    read_only: bool,
}

pub struct MsgArgs {}

impl Encode for MsgArgs {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        unimplemented!()
    }
}

pub struct Response {}

pub struct Error {}

impl Encode for Msg {
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        let mut dict = encoder.add_dict();
        dict.add("q", &self.query);
        dict.add("a", &self.args);
        dict.add("t", &self.txn_id.to_be_bytes()[..]);
        dict.add("y", &self.kind);
    }
}
