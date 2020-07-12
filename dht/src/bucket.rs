use crate::contact::{Contact, ContactRef};
use std::time::Instant;

#[derive(Debug)]
pub struct Bucket {
    pub live: Vec<Contact>,
    pub extra: Vec<Contact>,
    pub last_updated: Instant,
}

impl Bucket {
    // The 'K' constant in Kademlia algorithm
    pub const MAX_LEN: usize = 8;

    pub fn new() -> Self {
        Self {
            live: vec![],
            extra: vec![],
            last_updated: Instant::now(),
        }
    }

    pub fn get_contacts<'a>(&'a self, out: &mut Vec<ContactRef<'a>>) {
        for c in &self.live {
            if out.len() >= out.capacity() {
                break;
            }
            if !c.failed() {
                out.push(c.as_ref());
            }
        }
    }
}
