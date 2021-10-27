use crate::id::NodeId;
use crate::msg::TxnId;
use ben::DictEncoder;
use ben::Encode;

#[derive(Debug)]
pub struct Ping {
    pub txn_id: TxnId,
    pub id: NodeId,
}

impl Encode for Ping {
    fn encode(&self, buf: &mut Vec<u8>) {
        let mut d = DictEncoder::new(buf);

        let mut a = d.insert_dict("a");
        a.insert("id", self.id);
        a.finish();

        d.insert("q", "ping");
        d.insert("t", self.txn_id);
        d.insert("y", "q");
    }
}

#[derive(Debug)]
pub struct FindNode {
    pub txn_id: TxnId,
    pub id: NodeId,
    pub target: NodeId,
}

impl Encode for FindNode {
    fn encode(&self, buf: &mut Vec<u8>) {
        let mut d = DictEncoder::new(buf);

        let mut a = d.insert_dict("a");
        a.insert("id", self.id);
        a.insert("target", self.target);
        a.finish();

        d.insert("q", "find_node");
        d.insert("t", self.txn_id);
        d.insert("y", "q");
    }
}

#[derive(Debug)]
pub struct GetPeers {
    pub txn_id: TxnId,
    pub id: NodeId,
    pub info_hash: NodeId,
}

impl Encode for GetPeers {
    fn encode(&self, buf: &mut Vec<u8>) {
        let mut d = DictEncoder::new(buf);

        let mut a = d.insert_dict("a");
        a.insert("id", self.id);
        a.insert("info_hash", self.info_hash);
        a.finish();

        d.insert("q", "get_peers");
        d.insert("t", self.txn_id);
        d.insert("y", "q");
    }
}

#[derive(Debug)]
pub struct AnnouncePeer<'a> {
    pub txn_id: TxnId,
    pub id: NodeId,
    pub implied_port: bool,
    pub info_hash: NodeId,
    pub port: u16,
    pub token: &'a [u8],
}

impl Encode for AnnouncePeer<'_> {
    fn encode(&self, buf: &mut Vec<u8>) {
        let mut d = DictEncoder::new(buf);

        let mut a = d.insert_dict("a");
        a.insert("id", self.id);
        a.insert("implied_port", self.implied_port as i64);
        a.insert("info_hash", self.info_hash);
        a.insert("port", self.port as i64);
        a.insert("token", self.token);
        a.finish();

        d.insert("q", "announce_peer");
        d.insert("t", self.txn_id);
        d.insert("y", "q");
    }
}

// pub struct Error {
//     pub kind: ErrorKind,
//     pub description: String,
// }

// pub enum ErrorKind {
//     Generic,
//     Server,
//     Protocol,
//     MethodUnknown,
// }

// impl Encode for Error {
//     fn encode(&self, buf: &mut Vec<u8>) {
//         use ErrorKind::*;
//         let code = match self.kind {
//             Generic => 201,
//             Server => 202,
//             Protocol => 203,
//             MethodUnknown => 204,
//         };
//         enc.add_int(code);
//         enc.add_str(&self.description);
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;

    fn ascii_escape(buf: &[u8]) -> String {
        use std::ascii::escape_default;
        let v = buf.iter().flat_map(|&c| escape_default(c)).collect();
        // Safety: output of escape_default is valid UTF-8
        unsafe { String::from_utf8_unchecked(v) }
    }

    #[test]
    fn request_ping() {
        let request = Ping {
            txn_id: TxnId(10),
            id: NodeId::all(1),
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
            id: NodeId::all(1),
            target: NodeId::all(2),
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
            id: NodeId::all(1),
            info_hash: NodeId::all(2),
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
            id: NodeId::all(1),
            info_hash: NodeId::all(2),
            implied_port: false,
            port: 5000,
            token: &[0, 1, 2],
        };

        let encoded = request.encode_to_vec();
        let expected = b"d1:ad2:id20:\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x0112:implied_porti0e9:info_hash20:\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x024:porti5000e5:token3:\x00\x01\x02e1:q13:announce_peer1:t2:\x00\n1:y1:qe";
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
            id: NodeId::all(1),
            info_hash: NodeId::all(2),
            implied_port: true,
            port: 5000,
            token: &[0, 1, 2],
        };

        let encoded = request.encode_to_vec();
        let expected = b"d1:ad2:id20:\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x0112:implied_porti1e9:info_hash20:\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x02\x024:porti5000e5:token3:\x00\x01\x02e1:q13:announce_peer1:t2:\x00\n1:y1:qe";
        assert_eq!(
            encoded[..],
            expected[..],
            "\nExpected : {}\nActual   : {}",
            ascii_escape(expected),
            ascii_escape(&encoded)
        );
    }
}
