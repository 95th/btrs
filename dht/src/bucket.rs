use crate::contact::{Contact, ContactRef};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct Bucket {
    pub live: Vec<Contact>,
    pub extra: Vec<Contact>,
    pub last_updated: Instant,
}

impl Default for Bucket {
    fn default() -> Self {
        Self {
            live: vec![],
            extra: vec![],
            last_updated: Instant::now(),
        }
    }
}

impl Bucket {
    // The 'K' constant in Kademlia algorithm
    pub const MAX_LEN: usize = 8;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_contacts<'a>(&'a self, out: &mut Vec<ContactRef<'a>>) {
        self.live
            .iter()
            .filter(|c| !c.failed())
            .for_each(|c| out.push(c.as_ref()));
    }

    pub fn need_refresh(&self) -> bool {
        // Refresh every 15 mins
        const REFRESH_INTERVAL: Duration = Duration::from_secs(15 * 60);

        self.last_updated < Instant::now() - REFRESH_INTERVAL
    }
}
