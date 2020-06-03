use ben::Encode;
use dht::id::*;
use dht::msg::*;

fn main() {
    let mut buf = vec![];
    msg().encode(&mut buf);
    println!("{}", ascii_escape(&buf));
}

fn msg() -> Request {
    Request {
        txn_id: TxnId(123),
        query: Query::Ping {
            id: Box::new(NodeId::gen()),
        },
    }
}

pub fn ascii_escape(buf: &[u8]) -> String {
    use std::ascii::escape_default;
    let v = buf.iter().flat_map(|&c| escape_default(c)).collect();
    // Safety: output of escape_default is valid UTF-8
    unsafe { String::from_utf8_unchecked(v) }
}
