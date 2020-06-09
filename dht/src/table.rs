use crate::bucket::Bucket;
use crate::contact::{Contact, ContactRef};
use crate::id::NodeId;
use std::collections::{HashSet, VecDeque};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

pub enum AddContactStatus {
    Fail,
    Success,
    RequireSplit,
}

struct IpSet {
    ips_v4: HashSet<Ipv4Addr>,
    ips_v6: HashSet<Ipv6Addr>,
}

impl IpSet {
    fn new() -> Self {
        Self {
            ips_v4: HashSet::new(),
            ips_v6: HashSet::new(),
        }
    }

    fn exists(&self, ip: &IpAddr) -> bool {
        match ip {
            IpAddr::V4(ip) => self.ips_v4.contains(ip),
            IpAddr::V6(ip) => self.ips_v6.contains(ip),
        }
    }
}

pub struct RoutingTable {
    own_id: NodeId,
    buckets: VecDeque<Bucket>,
    router_nodes: HashSet<SocketAddr>,
    ips: IpSet,
}

impl RoutingTable {
    pub fn new(own_id: NodeId) -> Self {
        Self {
            own_id,
            buckets: VecDeque::new(),
            router_nodes: HashSet::new(),
            ips: IpSet::new(),
        }
    }

    pub fn add_router_node(&mut self, router: SocketAddr) {
        self.router_nodes.insert(router);
    }

    pub fn add_contact_impl(&mut self, contact: &ContactRef<'_>) -> AddContactStatus {
        if self.router_nodes.contains(&contact.addr) {
            return AddContactStatus::Fail;
        }

        if self.ips.exists(&contact.addr.ip()) {
            return AddContactStatus::Fail;
        }
        todo!()
    }

    pub fn update(&mut self, contact: &ContactRef<'_>) {
        if self.buckets.is_empty() {
            self.buckets.push_back(Bucket::new());
        }

        let bucket_no = self
            .own_id
            .xor_leading_zeros(contact.id)
            .min(self.buckets.len() - 1);

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

    pub fn len(&self) -> usize {
        self.buckets.iter().map(|b| b.live.len()).sum()
    }
}
