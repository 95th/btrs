use crate::bucket::Bucket;
use crate::contact::{Contact, ContactRef, ContactStatus};
use crate::id::NodeId;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub enum BucketResult {
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
    pub fn new(own_id: NodeId, router_nodes: Vec<SocketAddr>) -> Self {
        Self {
            own_id,
            buckets: vec![Bucket::new()],
            router_nodes: router_nodes.into_iter().collect(),
        }
    }

    pub fn pick_refresh_id(&self) -> Option<NodeId> {
        let timeout = Instant::now() - Duration::from_secs(15 * 60);
        let bucket_no = self.buckets.iter().position(|b| b.last_updated < timeout)?;
        trace!("Refresh bucket: {}", bucket_no);
        Some(NodeId::gen_lz(bucket_no))
    }

    pub fn add_router_node(&mut self, router: SocketAddr) {
        self.router_nodes.insert(router);
    }

    pub fn add_contact(&mut self, contact: &ContactRef<'_>) -> bool {
        let mut result = self.add_contact_impl(contact);
        loop {
            match result {
                BucketResult::Success => return true,
                BucketResult::Fail => return false,
                BucketResult::RequireSplit => {}
            }

            trace!("Split the buckets, before count: {}", self.buckets.len());
            self.split_bucket();
            trace!("Split the buckets, after count : {}", self.buckets.len());

            if let Some(last) = self.buckets.iter().last() {
                if last.live.len() > Bucket::MAX_LEN {
                    continue;
                }
            }

            result = self.add_contact_impl(contact);

            if let Some(last) = self.buckets.iter().last() {
                if last.live.is_empty() {
                    self.buckets.pop();
                    debug_assert_eq!(matches!(result, BucketResult::RequireSplit), false);
                }
            }
        }
    }

    pub fn find_closest<'a>(
        &'a self,
        target: &NodeId,
        out: &mut Vec<ContactRef<'a>>,
        count: usize,
    ) {
        let bucket_no = self.find_bucket(target);
        self.buckets[bucket_no].get_contacts(out);

        let len = self.buckets.len();
        let mut i = 1;

        while out.len() < count && (i <= bucket_no || bucket_no + i < len) {
            if i <= bucket_no {
                self.buckets[bucket_no - i].get_contacts(out);
            }
            if bucket_no + i < len {
                self.buckets[bucket_no + i].get_contacts(out);
            }
            i += 1;
        }

        out.sort_unstable_by_key(|c| target ^ c.id);
        out.truncate(count);
    }

    pub fn len(&self) -> usize {
        self.buckets.iter().map(|b| b.live.len()).sum()
    }

    pub fn len_extra(&self) -> usize {
        self.buckets.iter().map(|b| b.extra.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.buckets.iter().all(|b| b.live.is_empty())
    }

    pub fn find_contact(&mut self, id: &NodeId) -> Option<&mut Contact> {
        let idx = self.find_bucket(id);
        self.buckets[idx].live.iter_mut().find(|c| c.id == *id)
    }

    pub fn heard_from(&mut self, id: &NodeId) {
        let idx = self.find_bucket(id);
        let bucket = &mut self.buckets[idx];

        if let Some(c) = bucket.live.iter_mut().find(|c| c.id == *id) {
            c.status = ContactStatus::ALIVE | ContactStatus::QUERIED;
            c.clear_timeout();
            c.last_updated = Instant::now();
            bucket.last_updated = Instant::now();
        }
    }

    fn add_contact_impl(&mut self, contact: &ContactRef<'_>) -> BucketResult {
        if self.router_nodes.contains(&contact.addr) {
            return BucketResult::Fail;
        }

        // Don't add ourselves
        if self.own_id == *contact.id {
            return BucketResult::Fail;
        }

        let bucket_index = self.find_bucket(contact.id);
        let bucket_count = self.buckets.len();
        let can_split = { bucket_index + 1 == bucket_count && bucket_count < 159 };

        let bucket = &mut self.buckets[bucket_index];

        if let Some(c) = bucket.live.iter_mut().find(|c| c.id == *contact.id) {
            if c.addr != contact.addr {
                return BucketResult::Fail;
            } else {
                bucket.last_updated = Instant::now();
                c.clear_timeout();
                return BucketResult::Success;
            }
        }

        let maybe_extra = bucket
            .extra
            .iter_mut()
            .enumerate()
            .find(|(_, c)| c.id == *contact.id);

        if let Some((i, c)) = maybe_extra {
            if c.addr != contact.addr {
                return BucketResult::Fail;
            }

            c.clear_timeout();

            if c.is_pinged() && bucket.live.len() < Bucket::MAX_LEN {
                bucket.live.push(bucket.extra.remove(i));
                bucket.last_updated = Instant::now();
                return BucketResult::Success;
            }

            if can_split {
                return BucketResult::RequireSplit;
            }

            let result = self.replace_node_impl(contact, bucket_index);
            if !matches!(result, BucketResult::RequireSplit) {
                return result;
            }
        }

        let bucket = &mut self.buckets[bucket_index];

        if bucket.live.len() < Bucket::MAX_LEN {
            bucket.live.push(contact.as_owned());
            bucket.last_updated = Instant::now();
            return BucketResult::Success;
        }

        if can_split {
            return BucketResult::RequireSplit;
        }

        if bucket.extra.len() < Bucket::MAX_LEN * 2 {
            bucket.extra.push(contact.as_owned());
            bucket.last_updated = Instant::now();
            BucketResult::Success
        } else {
            BucketResult::Fail
        }
    }

    fn replace_node_impl(&mut self, contact: &ContactRef<'_>, bucket_index: usize) -> BucketResult {
        let bucket = &mut self.buckets[bucket_index];
        debug_assert!(bucket.live.len() >= Bucket::MAX_LEN);

        if replace_stale(&mut bucket.live, contact) || replace_stale(&mut bucket.extra, contact) {
            bucket.last_updated = Instant::now();
            BucketResult::Success
        } else {
            BucketResult::Fail
        }
    }

    fn split_bucket(&mut self) {
        if self.buckets.is_empty() {
            return;
        }

        let last_bucket_index = self.buckets.len() - 1;
        let bucket = &mut self.buckets[last_bucket_index];

        debug_assert!(bucket.live.len() >= Bucket::MAX_LEN);

        let mut new_bucket = Bucket::new();

        let mut i = 0;
        while i < bucket.live.len() {
            let bucket_index = bucket.live[i].id.xlz(&self.own_id);
            if bucket_index == last_bucket_index {
                i += 1;
                continue;
            }

            new_bucket.live.push(bucket.live.remove(i));
        }

        if bucket.live.len() > Bucket::MAX_LEN {
            bucket.extra.extend(bucket.live.drain(Bucket::MAX_LEN..));
        }

        let mut i = 0;
        while i < bucket.extra.len() {
            let bucket_index = bucket.extra[i].id.xlz(&self.own_id);
            if bucket_index == last_bucket_index {
                if bucket.live.len() >= Bucket::MAX_LEN {
                    i += 1;
                    continue;
                }
                bucket.live.push(bucket.extra.remove(i));
            } else {
                let contact = bucket.extra.remove(i);
                if new_bucket.live.len() < Bucket::MAX_LEN {
                    new_bucket.live.push(contact);
                } else {
                    new_bucket.extra.push(contact);
                }
            }
        }

        self.buckets.push(new_bucket);
    }

    fn find_bucket(&self, id: &NodeId) -> usize {
        let idx = self.own_id.xlz(id);
        let last_idx = self.buckets.len().checked_sub(1).unwrap();
        idx.min(last_idx)
    }
}

fn replace_stale(vec: &mut Vec<Contact>, contact: &ContactRef<'_>) -> bool {
    if let Some(most_stale) = vec.iter_mut().max_by_key(|c| c.fail_count()) {
        if most_stale.fail_count() > 0 {
            *most_stale = contact.as_owned();
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        let mut rt = RoutingTable::new(NodeId::all(0), vec![]);
        assert_eq!(rt.len(), 0);
        assert_eq!(rt.len_extra(), 0);
        assert_eq!(rt.buckets.len(), 1);

        let addr = SocketAddr::from(([0u8; 4], 100));

        // Add one contact
        assert!(rt.add_contact(&ContactRef {
            addr,
            id: &NodeId::all(1),
        }));
        assert_eq!(rt.len(), 1);
        assert_eq!(rt.len_extra(), 0);
        assert_eq!(rt.buckets.len(), 1);

        // Add the same contact again - Should add but size shouldn't change
        assert!(rt.add_contact(&ContactRef {
            addr,
            id: &NodeId::all(1),
        }));
        assert_eq!(rt.len(), 1);
        assert_eq!(rt.len_extra(), 0);
        assert_eq!(rt.buckets.len(), 1);

        // Add 7 more contacts, one bucket should be full now
        for i in 2..9 {
            assert!(rt.add_contact(&ContactRef {
                addr,
                id: &NodeId::all(i),
            }));
            assert_eq!(rt.len(), i as usize);
            assert_eq!(rt.len_extra(), 0);
            assert_eq!(rt.buckets.len(), 1);
        }

        // Add 1 more contacts - splits the bucket
        assert!(rt.add_contact(&ContactRef {
            addr,
            id: &NodeId::all(9),
        }));
        assert_eq!(rt.len(), 9);
        assert_eq!(rt.len_extra(), 0);
        assert_eq!(rt.buckets.len(), 6);
        assert_eq!(rt.buckets[4].live.len(), 2);
        assert_eq!(rt.buckets[5].live.len(), 7);

        // Add 6 more contacts - fill up bucket at index 4
        for i in 0..6 {
            let mut n = NodeId::all(9);
            n.0[19] = i as u8;
            assert!(rt.add_contact(&ContactRef { addr, id: &n }));
            assert_eq!(rt.len(), 10 + i);
            assert_eq!(rt.len_extra(), 0);
            assert_eq!(rt.buckets.len(), 6);
        }
        assert_eq!(rt.buckets[4].live.len(), 8);
        assert_eq!(rt.buckets[5].live.len(), 7);

        // Add 1 more contacts - goes into bucket index 4 extras
        let mut n = NodeId::all(9);
        n.0[19] = 6;
        assert!(rt.add_contact(&ContactRef { addr, id: &n }));
        assert_eq!(rt.len(), 15);
        assert_eq!(rt.len_extra(), 1);
        assert_eq!(rt.buckets.len(), 6);
        assert_eq!(rt.buckets[4].live.len(), 8);
        assert_eq!(rt.buckets[4].extra.len(), 1);
        assert_eq!(rt.buckets[5].live.len(), 7);
    }

    #[test]
    fn test_closest() {
        let mut table = RoutingTable::new(NodeId::all(0), vec![]);
        let addr = SocketAddr::from(([0u8; 4], 100));

        fn node(idx: usize) -> NodeId {
            let mut buf = [0; 20];
            buf[idx] = 1;
            NodeId::from(buf)
        }

        for i in 0..20 {
            let added = table.add_contact(&ContactRef { id: &node(i), addr });
            assert!(added, "Adding contact failed at {}", i);
        }

        let mut closest = Vec::with_capacity(20);
        table.find_closest(&NodeId::all(1), &mut closest, 20);

        let mut closest_iter = closest.into_iter();
        for i in 0..20 {
            assert_eq!(closest_iter.next().unwrap().id, &node(i));
        }

        assert!(closest_iter.next().is_none());
    }
}
