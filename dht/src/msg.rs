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
    response: Option<Response>,
    error: Option<Error>,
    ip: NodeAddr,
    read_only: bool,
}

pub struct MsgArgs {
    id: NodeId,
    info_hash: NodeId,
    target: NodeId,
    token: Vec<u8>,
    port: u16,
    implied_port: bool,
}

impl Encode for MsgArgs {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        let mut dict = enc.add_dict();
        dict.add("id", &self.id.as_bytes()[..]);
        dict.add("info_hash", &self.info_hash.as_bytes()[..]);
        dict.add("target", &self.target.as_bytes()[..]);
        dict.add("token", &self.token[..]);
        dict.add("port", self.port as i64);
        dict.add("implied_port", if self.implied_port { 1 } else { 0 });
        dict.finish();
    }
}

pub struct Response {}

impl Encode for Response {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        unimplemented!()
    }
}

pub struct Error {}

impl Encode for Error {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        unimplemented!()
    }
}

struct NodeAddr(SocketAddr);

impl Encode for NodeAddr {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        match self.0 {
            SocketAddr::V4(addr) => {
                let mut buf = [0; 6];
                buf.copy_from_slice(&addr.ip().octets());
                buf[4..].copy_from_slice(&addr.port().to_be_bytes());
                enc.add_bytes(&buf[..]);
            }
            SocketAddr::V6(addr) => {
                let mut buf = [0; 18];
                buf.copy_from_slice(&addr.ip().octets());
                buf[16..].copy_from_slice(&addr.port().to_be_bytes());
                enc.add_bytes(&buf[..]);
            }
        }
    }
}

impl Encode for Msg {
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        let mut dict = encoder.add_dict();
        dict.add("q", &self.query);
        dict.add("a", &self.args);
        dict.add("t", &self.txn_id.to_be_bytes()[..]);
        dict.add("y", &self.kind);

        if let Some(response) = &self.response {
            dict.add("r", response);
        }

        if let Some(error) = &self.error {
            dict.add("e", error);
        }

        dict.add("ip", &self.ip);
        dict.add("ro", if self.read_only { 1 } else { 0 });
    }
}
