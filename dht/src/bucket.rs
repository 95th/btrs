use crate::contact::Contact;
use std::collections::VecDeque;

const BUCKET_SIZE: usize = 8;

pub struct Bucket {
    live: VecDeque<Contact>,
    replacement: VecDeque<Contact>,
}

impl Bucket {
    pub fn new() -> Self {
        Bucket {
            live: VecDeque::new(),
            replacement: VecDeque::new(),
        }
    }

    pub fn update(&mut self, contact: Contact) {
        let maybe_idx = self.live.iter().position(|c| c.id == contact.id);

        if let Some(idx) = maybe_idx {
            let contact = self.live.remove(idx).unwrap();
            self.live.push_front(contact);
        } else if self.live.len() < BUCKET_SIZE {
            self.live.push_front(contact);
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
