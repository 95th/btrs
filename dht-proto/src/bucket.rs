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

    pub fn replace_node(&mut self, contact: Contact) -> bool {
        debug_assert!(self.live.len() >= Bucket::MAX_LEN);

        let maybe_stale = find_stale(&mut self.live).or_else(|| find_stale(&mut self.extra));

        if let Some(stale) = maybe_stale {
            *stale = contact;
            return true;
        }

        false
    }
}

fn find_stale(contacts: &mut Vec<Contact>) -> Option<&mut Contact> {
    contacts
        .iter_mut()
        .filter(|c| c.fail_count() > 0)
        .max_by_key(|c| c.fail_count())
}
