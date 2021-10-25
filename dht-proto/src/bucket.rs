use crate::contact::Contact;

#[derive(Debug, Default, Clone)]
pub struct Bucket {
    pub live: Vec<Contact>,
    pub extra: Vec<Contact>,
}

impl Bucket {
    // The 'K' constant in Kademlia algorithm
    pub const MAX_LEN: usize = 8;

    pub const fn new() -> Self {
        Self {
            live: Vec::new(),
            extra: Vec::new(),
        }
    }

    pub fn is_full(&self) -> bool {
        self.live.len() >= Self::MAX_LEN && self.extra.len() >= Self::MAX_LEN
    }

    pub fn get_contacts<'a>(&'a self, out: &mut Vec<&'a Contact>) {
        self.live
            .iter()
            .filter(|c| !c.failed())
            .for_each(|c| out.push(c));
    }

    pub fn replace_node(&mut self, contact: &Contact) -> bool {
        debug_assert!(self.live.len() >= Bucket::MAX_LEN);

        replace_stale(&mut self.live, contact) || replace_stale(&mut self.extra, contact)
    }
}

fn replace_stale(vec: &mut Vec<Contact>, contact: &Contact) -> bool {
    if let Some(most_stale) = vec.iter_mut().max_by_key(|c| c.fail_count()) {
        if most_stale.fail_count() > 0 {
            *most_stale = contact.clone();
            return true;
        }
    }
    false
}
