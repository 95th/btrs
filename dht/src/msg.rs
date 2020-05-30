use crate::id::NodeId;
use anyhow::Context;
use ben::{Encode, Encoder, Node, Token};
use std::convert::TryInto;
use std::net::SocketAddr;

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
        id: NodeId,
    },
    FindNode {
        id: NodeId,
        target: NodeId,
    },
    GetPeers {
        id: NodeId,
        info_hash: NodeId,
    },
    AnnouncePeer {
        id: NodeId,
        implied_port: bool,
        info_hash: NodeId,
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
                dict.add("implied_port", if *implied_port { 1 } else { 0 });
                dict.add("port", *port as i64);
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
                dict.get_dict(b"e").context("Error data not found")?;
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
