use crate::id::NodeId;
use ben::Encoder;

type TxnId = u16;

pub trait Msg {
    fn encode(&self, txn_id: TxnId) -> Vec<u8>;
}

pub struct Ping(NodeId);

impl Msg for Ping {
    fn encode(&self, txn_id: TxnId) -> Vec<u8> {
        let mut data = vec![];

        let mut dict = data.add_dict();
        dict.add_bytes("t", &txn_id.to_be_bytes());
        dict.add_str("y", "q");
        dict.add_str("q", "ping");

        let mut a = dict.add_dict("a");
        a.add_bytes("id", self.0.as_bytes());
        a.finish();

        dict.finish();
        data
    }
}
