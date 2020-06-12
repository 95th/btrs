use crate::bucket::{Bucket, BUCKET_SIZE};
use crate::contact::{Contact, ContactRef};
use crate::id::NodeId;
use std::collections::{HashSet, VecDeque};
use std::net::SocketAddr;

#[derive(Debug)]
pub enum BucketStatus {
    Fail,
    Success,
    RequireSplit,
}

#[derive(Debug)]
pub struct RoutingTable {
    pub own_id: NodeId,
    pub buckets: Vec<Bucket>,
    pub router_nodes: HashSet<SocketAddr>,
}

impl RoutingTable {
    pub fn new(own_id: NodeId) -> Self {
        Self {
            own_id,
            buckets: Vec::new(),
            router_nodes: HashSet::new(),
        }
    }

    pub fn add_router_node(&mut self, router: SocketAddr) {
        self.router_nodes.insert(router);
    }

    pub fn add_contact(&mut self, contact: &ContactRef<'_>) -> bool {
        let mut status = self.add_contact_impl(contact);
        loop {
            trace!("Adding contact, got: {:?}", status);
            match status {
                BucketStatus::Success => return true,
                BucketStatus::Fail => return false,
                BucketStatus::RequireSplit => {}
            }

            trace!("Split the buckets, before count: {}", self.buckets.len());
            self.split_bucket();
            trace!("Split the buckets, after count : {}", self.buckets.len());

            if self.buckets.len() > 50 {
                status = self.add_contact_impl(contact);
                trace!("Try adding again, got: {:?}", status);
                return matches!(status, BucketStatus::Success);
            }

            if let Some(last) = self.buckets.iter().last() {
                if last.live.len() > BUCKET_SIZE {
                    continue;
                }
            }

            status = self.add_contact_impl(contact);

            if let Some(last) = self.buckets.iter().last() {
                if last.live.is_empty() {
                    self.buckets.pop();
                    debug_assert_eq!(matches!(status, BucketStatus::RequireSplit), false);
                }
            }
        }
    }

    pub fn find_closest(&self, target: &NodeId, count: usize) -> Vec<&Contact> {
        let mut contacts = Vec::with_capacity(count);

        let bucket_no = self.own_id.xlz(target);
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

    pub fn len(&self) -> usize {
        self.buckets.iter().map(|b| b.live.len()).sum()
    }

    pub fn len_extra(&self) -> usize {
        self.buckets.iter().map(|b| b.extra.len()).sum()
    }

    fn add_contact_impl(&mut self, contact: &ContactRef<'_>) -> BucketStatus {
        if self.router_nodes.contains(&contact.addr) {
            return BucketStatus::Fail;
        }

        if self.own_id == *contact.id {
            return BucketStatus::Fail;
        }

        let bucket_idx = self.find_bucket(contact.id);
        let Bucket { live, extra } = &mut self.buckets[bucket_idx];

        if let Some(c) = live.iter_mut().find(|c| c.id == *contact.id) {
            if c.addr != contact.addr {
                return BucketStatus::Fail;
            } else {
                // TODO: Update timeouts etc
                return BucketStatus::Success;
            }
        }

        let maybe_extra = extra
            .iter_mut()
            .enumerate()
            .find(|(_, c)| c.id == *contact.id);

        if let Some((i, c)) = maybe_extra {
            return if c.addr != contact.addr {
                BucketStatus::Fail
            } else {
                // TODO: Update timeouts etc
                if live.len() < BUCKET_SIZE {
                    live.push(extra.remove(i));
                    BucketStatus::Success
                } else {
                    BucketStatus::RequireSplit
                }
            };
        }

        if live.len() < BUCKET_SIZE {
            live.push(contact.as_owned());
            BucketStatus::Success
        } else {
            BucketStatus::RequireSplit
        }
    }

    fn split_bucket(&mut self) {
        if self.buckets.is_empty() {
            return;
        }

        let curr_bkt_idx = self.buckets.len() - 1;
        let Bucket { live, extra } = &mut self.buckets[curr_bkt_idx];

        debug_assert!(live.len() >= BUCKET_SIZE);

        let mut new_bucket = Bucket::new();

        let mut i = 0;
        while i < live.len() {
            let bkt_idx = live[i].id.xlz(&self.own_id);
            if bkt_idx == curr_bkt_idx {
                i += 1;
                continue;
            }

            new_bucket.live.push(live.remove(i));
        }

        if live.len() > BUCKET_SIZE {
            extra.extend(live.drain(BUCKET_SIZE..));
        }

        let mut i = 0;
        while i < extra.len() {
            let bkt_idx = extra[i].id.xlz(&self.own_id);
            if bkt_idx == curr_bkt_idx {
                if live.len() >= BUCKET_SIZE {
                    i += 1;
                    continue;
                }
                live.push(extra.remove(i));
            } else {
                let contact = extra.remove(i);
                if new_bucket.live.len() < BUCKET_SIZE {
                    new_bucket.live.push(contact);
                } else {
                    new_bucket.extra.push(contact);
                }
            }
        }

        self.buckets.push(new_bucket);
    }

    fn find_bucket(&mut self, id: &NodeId) -> usize {
        if self.buckets.is_empty() {
            self.buckets.push(Bucket::new());
        }

        let bucket_idx = self.own_id.xlz(id).min(self.buckets.len() - 1);

        bucket_idx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        let mut rt = RoutingTable::new(NodeId::of_byte(0));
        assert_eq!(rt.len(), 0);
        assert_eq!(rt.len_extra(), 0);
        assert_eq!(rt.buckets.len(), 0);

        let addr = SocketAddr::from(([0u8; 4], 100));

        // Add one contact
        assert!(rt.add_contact(&ContactRef {
            addr,
            id: &NodeId::of_byte(1),
        }));
        assert_eq!(rt.len(), 1);
        assert_eq!(rt.len_extra(), 0);
        assert_eq!(rt.buckets.len(), 1);

        // Add the same contact again - Should add but size shouldn't change
        assert!(rt.add_contact(&ContactRef {
            addr,
            id: &NodeId::of_byte(1),
        }));
        assert_eq!(rt.len(), 1);
        assert_eq!(rt.len_extra(), 0);
        assert_eq!(rt.buckets.len(), 1);

        // Add 7 more contacts, one bucket should be full now
        for i in 2..9 {
            assert!(rt.add_contact(&ContactRef {
                addr,
                id: &NodeId::of_byte(i),
            }));
            assert_eq!(rt.len(), i as usize);
            assert_eq!(rt.len_extra(), 0);
            assert_eq!(rt.buckets.len(), 1);
        }

        // Add 1 more contacts - splits the bucket
        assert!(rt.add_contact(&ContactRef {
            addr,
            id: &NodeId::of_byte(9),
        }));
        assert_eq!(rt.len(), 9);
        assert_eq!(rt.len_extra(), 0);
        assert_eq!(rt.buckets.len(), 6);
        assert_eq!(rt.buckets[4].live.len(), 2);
        assert_eq!(rt.buckets[5].live.len(), 7);

        // Add 6 more contacts - fill up bucket at index 4
        for i in 0..6 {
            let mut n = NodeId::of_byte(9);
            n.0[19] = i as u8;
            assert!(rt.add_contact(&ContactRef { addr, id: &n }));
            assert_eq!(rt.len(), 10 + i);
            assert_eq!(rt.len_extra(), 0);
            assert_eq!(rt.buckets.len(), 6);
        }
        assert_eq!(rt.buckets[4].live.len(), 8);
        assert_eq!(rt.buckets[5].live.len(), 7);
    }
}
