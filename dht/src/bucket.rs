use crate::contact::{Contact, ContactRef};
use std::collections::VecDeque;

pub(crate) const BUCKET_SIZE: usize = 8;

#[derive(Debug)]
pub struct Bucket {
    pub live: VecDeque<Contact>,
    pub extra: VecDeque<Contact>,
}

impl Bucket {
    pub fn new() -> Self {
        Bucket {
            live: VecDeque::new(),
            extra: VecDeque::new(),
        }
    }

    pub fn update(&mut self, contact: &ContactRef<'_>) {
        let maybe_idx = self.live.iter().position(|c| c.id == *contact.id);

        if let Some(idx) = maybe_idx {
            let existing = self.live.remove(idx).unwrap();
            self.live.push_front(existing);
        } else if self.live.len() < BUCKET_SIZE {
            self.live.push_front(contact.as_owned());
        } else {
            // TODO: Handle insertion when the list is full by evicting old elements if
            // they don't respond to a ping
        }
    }

    pub fn get_contacts<'a>(&'a self, contacts: &mut Vec<&'a Contact>, count: usize) {
        for c in &self.live {
            if contacts.len() < count {
                contacts.push(c);
            } else {
                break;
            }
        }
    }
}
