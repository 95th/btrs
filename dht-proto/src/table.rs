use crate::contact::{CompactNodes, Contact, ContactStatus};
use crate::id::NodeId;
use crate::msg::recv::Response;
use crate::{bucket::Bucket, server::ClientRequest};

use std::collections::HashSet;
use std::mem::MaybeUninit;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

const BUCKETS: usize = 160;

#[derive(Debug)]
pub struct RoutingTable {
    pub root_id: NodeId,
    pub buckets: [Bucket; BUCKETS],
    pub timeouts: [Instant; BUCKETS],
    pub router_nodes: HashSet<SocketAddr>,
}

impl RoutingTable {
    pub fn new(root_id: NodeId, router_nodes: Vec<SocketAddr>, now: Instant) -> Self {
        // Bucket is not `Copy`. So create it using an uninitialized array
        let buckets = unsafe {
            let mut buckets = MaybeUninit::<[Bucket; BUCKETS]>::uninit();
            let ptr = buckets.as_mut_ptr().cast::<Bucket>();
            for i in 0..BUCKETS {
                ptr.add(i).write(Bucket::new());
            }
            buckets.assume_init()
        };

        Self {
            root_id,
            buckets,
            timeouts: [next_timeout(now); BUCKETS],
            router_nodes: router_nodes.into_iter().collect(),
        }
    }

    pub fn next_timeout(&self) -> Option<Instant> {
        self.timeouts.iter().min().copied()
    }

    pub fn next_refresh(&mut self, now: Instant) -> Option<ClientRequest> {
        let idx = self.timeouts.iter().position(|t| now > *t)?;
        log::trace!("Refresh bucket: {}", idx);

        self.timeouts[idx] = next_timeout(now);
        let bucket = &mut self.buckets[idx];

        if bucket.is_full() {
            let c = bucket
                .live
                .iter()
                .chain(bucket.extra.iter())
                .max_by_key(|c| c.fail_count())?;

            Some(ClientRequest::Ping {
                id: c.id,
                addr: c.addr,
            })
        } else {
            let id = NodeId::gen_leading_zeros(idx);
            Some(ClientRequest::Bootstrap { target: id })
        }
    }

    pub fn add_contact(&mut self, mut contact: Contact, now: Instant) -> bool {
        // Don't add router nodes
        if self.router_nodes.contains(&contact.addr) {
            return false;
        }

        // Don't add ourselves
        if self.root_id == contact.id {
            return false;
        }

        let idx = self.idx_of(contact.id);
        let bucket = &mut self.buckets[idx];
        let timeout = &mut self.timeouts[idx];

        if let Some(c) = bucket.live.iter_mut().find(|c| c.id == contact.id) {
            if c.addr != contact.addr {
                return false;
            }

            c.set_confirmed();
            *timeout = next_timeout(now);
            return true;
        }

        let maybe_extra = bucket
            .extra
            .iter_mut()
            .enumerate()
            .find(|(_, c)| c.id == contact.id);

        if let Some((i, c)) = maybe_extra {
            if c.addr != contact.addr {
                return false;
            }

            c.set_confirmed();
            contact = bucket.extra.remove(i);
        }

        if bucket.live.len() < Bucket::MAX_LEN {
            if bucket.live.is_empty() {
                bucket.live.reserve(Bucket::MAX_LEN);
            }
            bucket.live.push(contact);
            *timeout = next_timeout(now);
            return true;
        }

        if contact.is_confirmed() {
            return if bucket.replace_node(contact) {
                *timeout = next_timeout(now);
                true
            } else {
                false
            };
        }

        // if we can't replace anything in the live buckets, then try to insert
        // into the replacement bucket

        // if we don't have any identified stale nodes in
        // the bucket, and the bucket is full, we have to
        // cache this node and wait until some node fails
        // and then replace it.
        if let Some(c) = bucket.extra.iter_mut().find(|c| c.addr == contact.addr) {
            c.set_pinged();
            return true;
        }

        if bucket.extra.len() >= Bucket::MAX_LEN {
            if let Some(i) = bucket.extra.iter().position(|c| !c.is_pinged()) {
                bucket.extra.remove(i);
            } else {
                return if bucket.replace_node(contact) {
                    *timeout = next_timeout(now);
                    true
                } else {
                    false
                };
            }
        }

        if bucket.extra.is_empty() {
            bucket.extra.reserve(Bucket::MAX_LEN);
        }
        bucket.extra.push(contact);
        *timeout = next_timeout(now);
        true
    }

    pub fn find_closest(&self, target: NodeId, count: usize) -> Vec<&Contact> {
        let mut out = Vec::with_capacity(count);

        let idx = self.idx_of(target);
        self.buckets[idx].get_contacts(&mut out);

        let mut i = 1;

        while out.len() < count && (i <= idx || idx + i < BUCKETS) {
            if i <= idx {
                self.buckets[idx - i].get_contacts(&mut out);
            }
            if idx + i < BUCKETS {
                self.buckets[idx + i].get_contacts(&mut out);
            }
            i += 1;
        }

        out
    }

    pub fn read_nodes_with<F>(
        &mut self,
        response: &Response,
        now: Instant,
        mut f: F,
    ) -> anyhow::Result<()>
    where
        F: FnMut(&Contact),
    {
        if let Some(nodes) = response.body.get_bytes("nodes") {
            for c in CompactNodes::<4>::new(nodes)? {
                f(&c);
                self.add_contact(c, now);
            }
        }

        if let Some(nodes6) = response.body.get_bytes("nodes6") {
            for c in CompactNodes::<16>::new(nodes6)? {
                f(&c);
                self.add_contact(c, now);
            }
        }

        log::trace!("Live: {}, Extra: {}", self.len(), self.len_extra());

        Ok(())
    }

    pub fn len(&self) -> usize {
        self.buckets.iter().map(|b| b.live.len()).sum()
    }

    pub fn len_extra(&self) -> usize {
        self.buckets.iter().map(|b| b.extra.len()).sum()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.buckets.iter().all(|b| b.live.is_empty())
    }

    pub fn find_contact(&mut self, id: NodeId) -> Option<&mut Contact> {
        let idx = self.idx_of(id);
        self.buckets[idx].live.iter_mut().find(|c| c.id == id)
    }

    pub fn failed(&mut self, id: NodeId) {
        if let Some(c) = self.find_contact(id) {
            c.timed_out();
        }
    }

    pub fn heard_from(&mut self, id: NodeId, now: Instant) {
        let idx = self.idx_of(id);
        let bucket = &mut self.buckets[idx];

        if let Some(c) = bucket.live.iter_mut().find(|c| c.id == id) {
            c.status = ContactStatus::ALIVE | ContactStatus::QUERIED;
            c.clear_timeout();
            self.timeouts[idx] = next_timeout(now);
        }
    }

    fn idx_of(&self, id: NodeId) -> usize {
        self.root_id.xor_leading_zeros(id).min(BUCKETS - 1)
    }
}

fn next_timeout(instant: Instant) -> Instant {
    // 15 mins
    const BUCKET_TIMEOUT: Duration = Duration::from_secs(15 * 60);

    instant + BUCKET_TIMEOUT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        let mut table = RoutingTable::new(NodeId::all(0), vec![], Instant::now());
        assert!(table.is_empty());
        assert_eq!(table.len_extra(), 0);

        let addr = SocketAddr::from(([0u8; 4], 100));

        // Add one contact
        let n = NodeId::all(1);
        assert!(table.add_contact(Contact::new(n, addr), Instant::now()));
        assert_eq!(table.len(), 1);
        assert_eq!(table.len_extra(), 0);

        // Add the same contact again - Should add but size shouldn't change
        assert!(table.add_contact(Contact::new(n, addr), Instant::now()));
        assert_eq!(table.len(), 1);
        assert_eq!(table.len_extra(), 0);

        // Add 7 more contacts
        for i in 2..9 {
            let n = NodeId::all(i);
            assert!(table.add_contact(Contact::new(n, addr), Instant::now()));
            assert_eq!(table.len(), i as usize);
            assert_eq!(table.len_extra(), 0);
        }

        assert_eq!(table.len(), 8);
        assert_eq!(table.len_extra(), 0);
        assert_eq!(table.buckets[8].live.len(), 0);
        assert_eq!(table.buckets[7].live.len(), 1);
        assert_eq!(table.buckets[6].live.len(), 2);
        assert_eq!(table.buckets[5].live.len(), 4);
        assert_eq!(table.buckets[4].live.len(), 1);

        // Add 1 more contact
        let n = NodeId::all(9);
        assert!(table.add_contact(Contact::new(n, addr), Instant::now()));

        assert_eq!(table.len(), 9);
        assert_eq!(table.len_extra(), 0);
        assert_eq!(table.buckets[4].live.len(), 2);

        // Add 6 more contacts
        for i in 0..6 {
            let mut n = NodeId::all(9);
            n[19] = i as u8;
            assert!(table.add_contact(Contact::new(n, addr), Instant::now()));
            assert_eq!(table.len(), 10 + i);
            assert_eq!(table.len_extra(), 0);
        }

        assert_eq!(table.len(), 15);
        assert_eq!(table.len_extra(), 0);
        assert_eq!(table.buckets[4].live.len(), 8);

        // Add 1 more contacts - goes into bucket index 4 extras
        let mut n = NodeId::all(9);
        n[19] = 6;
        assert!(table.add_contact(Contact::new(n, addr), Instant::now()));
        assert_eq!(table.len(), 15);
        assert_eq!(table.len_extra(), 1);
        assert_eq!(table.buckets[4].live.len(), 8);
        assert_eq!(table.buckets[4].extra.len(), 1);
        assert_eq!(table.buckets[3].live.len(), 0);
    }

    #[test]
    fn test_closest() {
        let mut table = RoutingTable::new(NodeId::all(0), vec![], Instant::now());
        let addr = SocketAddr::from(([0u8; 4], 100));

        fn node(idx: usize) -> NodeId {
            let mut buf = [0; 20];
            buf[idx] = 1;
            NodeId::from(buf)
        }

        for i in 0..20 {
            let added = table.add_contact(Contact::new(node(i), addr), Instant::now());
            assert!(added, "Adding contact failed at {}", i);
        }

        let closest = table.find_closest(NodeId::all(1), 20);

        let mut closest_iter = closest.into_iter();
        for i in 0..20 {
            assert_eq!(closest_iter.next().unwrap().id, node(i));
        }

        assert!(closest_iter.next().is_none());
    }
}
