use crate::contact::Contact;

pub(crate) const BUCKET_SIZE: usize = 8;

#[derive(Debug)]
pub struct Bucket {
    pub live: Vec<Contact>,
    pub extra: Vec<Contact>,
}

impl Bucket {
    pub fn new() -> Self {
        Bucket {
            live: Vec::new(),
            extra: Vec::new(),
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
