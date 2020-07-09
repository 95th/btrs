use crate::bucket::Bucket;
use crate::contact::{Contact, ContactRef};
use crate::id::NodeId;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;

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
    pub tokens: HashMap<NodeId, Vec<u8>>,
    pub router_nodes: HashSet<SocketAddr>,
}

impl RoutingTable {
    pub fn new(own_id: NodeId) -> Self {
        Self {
            own_id,
            buckets: vec![Bucket::new()],
            tokens: HashMap::new(),
            router_nodes: HashSet::new(),
        }
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
        &'a mut self,
        target: &NodeId,
        out: &mut Vec<&'a mut Contact>,
        count: usize,
    ) {
        let bucket_no = self.find_bucket(target);

        // Safety: We're just getting mutable references to contacts in the different buckets.
        // We're not adding or removing any bucket to the buckets vector itself. The mutable
        // borrow of each bucket is exclusive.
        unsafe {
            let len = self.buckets.len();
            let ptr = self.buckets.as_mut_ptr();

            let bucket = &mut *ptr.add(bucket_no);
            bucket.get_contacts(out, count);

            let mut i = 1;

            while out.len() < count && (i <= bucket_no || bucket_no + i < len) {
                if i <= bucket_no {
                    let bucket = &mut *ptr.add(bucket_no - i);
                    bucket.get_contacts(out, count);
                }

                if bucket_no + i < len {
                    let bucket = &mut *ptr.add(bucket_no + i);
                    bucket.get_contacts(out, count);
                }

                i += 1;
            }
        }

        out.sort_unstable_by_key(|c| target ^ &c.id);
    }

    pub fn len(&self) -> usize {
        self.buckets.iter().map(|b| b.live.len()).sum()
    }

    pub fn len_extra(&self) -> usize {
        self.buckets.iter().map(|b| b.extra.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn find_contact_by_id(&mut self, id: &NodeId) -> Option<&mut Contact> {
        let idx = self.find_bucket(id);
        self.buckets[idx].live.iter_mut().find(|c| c.id == *id)
    }

    pub fn find_contact(&mut self, addr: &SocketAddr) -> Option<&mut Contact> {
        self.buckets
            .iter_mut()
            .find_map(|b| b.live.iter_mut().find(|c| c.addr == *addr))
    }

    pub fn min_dist(&self, target: &NodeId) -> NodeId {
        self.buckets
            .iter()
            .flat_map(|b| b.live.iter())
            .map(|c| &c.id ^ target)
            .min()
            .unwrap_or_else(|| target.clone())
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

        let Bucket { live, extra } = &mut self.buckets[bucket_index];

        if let Some(c) = live.iter_mut().find(|c| c.id == *contact.id) {
            if c.addr != contact.addr {
                return BucketResult::Fail;
            } else {
                c.clear_timeout();
                return BucketResult::Success;
            }
        }

        let maybe_extra = extra
            .iter_mut()
            .enumerate()
            .find(|(_, c)| c.id == *contact.id);

        if let Some((i, c)) = maybe_extra {
            if c.addr != contact.addr {
                return BucketResult::Fail;
            }

            c.clear_timeout();

            if c.is_pinged() && live.len() < Bucket::MAX_LEN {
                live.push(extra.remove(i));
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

        let Bucket { live, extra } = &mut self.buckets[bucket_index];

        if live.len() < Bucket::MAX_LEN {
            live.push(contact.as_owned());
            return BucketResult::Success;
        }

        if can_split {
            return BucketResult::RequireSplit;
        }

        extra.push(contact.as_owned());
        BucketResult::Success
    }

    fn replace_node_impl(&mut self, contact: &ContactRef<'_>, bucket_index: usize) -> BucketResult {
        let Bucket { live, extra } = &mut self.buckets[bucket_index];
        debug_assert!(live.len() >= Bucket::MAX_LEN);

        if replace_stale(live, contact) || replace_stale(extra, contact) {
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
        let Bucket { live, extra } = &mut self.buckets[last_bucket_index];

        debug_assert!(live.len() >= Bucket::MAX_LEN);

        let mut new_bucket = Bucket::new();

        let mut i = 0;
        while i < live.len() {
            let bucket_index = live[i].id.xlz(&self.own_id);
            if bucket_index == last_bucket_index {
                i += 1;
                continue;
            }

            new_bucket.live.push(live.remove(i));
        }

        if live.len() > Bucket::MAX_LEN {
            extra.extend(live.drain(Bucket::MAX_LEN..));
        }

        let mut i = 0;
        while i < extra.len() {
            let bucket_index = extra[i].id.xlz(&self.own_id);
            if bucket_index == last_bucket_index {
                if live.len() >= Bucket::MAX_LEN {
                    i += 1;
                    continue;
                }
                live.push(extra.remove(i));
            } else {
                let contact = extra.remove(i);
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
        let mut rt = RoutingTable::new(NodeId::of_byte(0));
        assert_eq!(rt.len(), 0);
        assert_eq!(rt.len_extra(), 0);
        assert_eq!(rt.buckets.len(), 1);

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

        // Add 1 more contacts - goes into bucket index 4 extras
        let mut n = NodeId::of_byte(9);
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
        let mut table = RoutingTable::new(NodeId::of_byte(0));
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

        let closest = &mut vec![];
        table.find_closest(&NodeId::of_byte(1), closest, 20);

        let mut closest_iter = closest.into_iter();
        for i in 0..20 {
            assert_eq!(closest_iter.next().unwrap().id, node(i));
        }

        assert!(closest_iter.next().is_none());
    }
}
