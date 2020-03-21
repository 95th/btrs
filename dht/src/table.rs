use crate::entry::NodeEntry;
use crate::id::NodeId;
use std::collections::VecDeque;

const BUCKET_SIZE: usize = 20;

pub type Bucket = Vec<NodeEntry>;

pub struct RoutingNode {
    pub replacements: Bucket,
    pub live: Bucket,
}

pub struct Contact {
    id: NodeId,
}

pub struct RoutingTable {
    node: NodeId,
    buckets: VecDeque<VecDeque<Contact>>,
}

impl RoutingTable {
    pub fn new(node: NodeId) -> Self {
        let mut buckets = VecDeque::with_capacity(NodeId::LEN * 8);
        for _ in 0..buckets.capacity() {
            buckets.push_back(VecDeque::new());
        }
        Self { node, buckets }
    }

    pub fn update(&mut self, contact: Contact) {
        let table_node = &self.node;
        let bucket_no = (table_node ^ &contact.id).leading_zeros();
        let bucket = &mut self.buckets[bucket_no];

        let maybe_idx = bucket.iter().position(|c| c.id == contact.id);

        if let Some(idx) = maybe_idx {
            let contact = bucket.remove(idx).unwrap();
            bucket.push_front(contact);
        } else if bucket.len() < BUCKET_SIZE {
            bucket.push_front(contact);
        } else {
            // TODO: Handle insertion when the list is full by evicting old elements if
            // they don't respond to a ping
        }
    }
}
