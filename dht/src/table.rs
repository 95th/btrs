use crate::bucket::Bucket;
use crate::contact::{Contact, ContactRef};
use crate::id::NodeId;
use std::collections::VecDeque;

pub struct RoutingTable {
    own_id: NodeId,
    buckets: VecDeque<Bucket>,
}

impl RoutingTable {
    pub fn new(own_id: NodeId) -> Self {
        Self {
            own_id,
            buckets: VecDeque::new(),
        }
    }

    pub fn update(&mut self, contact: ContactRef<'_>) {
        let bucket_no = self.own_id.xor_leading_zeros(contact.id);
        self.buckets[bucket_no].update(contact);
    }

    pub fn find_closest(&self, target: &NodeId, count: usize) -> Vec<&Contact> {
        let mut contacts = Vec::with_capacity(count);

        let bucket_no = self.own_id.xor_leading_zeros(target);
        self.buckets[bucket_no].get_contacts(&mut contacts, count);

        let mut i = 1;
        while contacts.len() < count && (i < bucket_no || bucket_no + i < self.buckets.len()) {
            if i < bucket_no {
                self.buckets[bucket_no - i].get_contacts(&mut contacts, count);
            }

            if bucket_no + i < self.buckets.len() {
                self.buckets[bucket_no + i].get_contacts(&mut contacts, count);
            }

            i += 1;
        }

        contacts.sort_unstable_by_key(|c| &c.id ^ target);
        contacts
    }
}
