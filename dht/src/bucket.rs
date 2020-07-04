use crate::contact::Contact;

#[derive(Debug, Default)]
pub struct Bucket {
    pub live: Vec<Contact>,
    pub extra: Vec<Contact>,
}

impl Bucket {
    // The 'K' constant in Kademlia algorithm
    pub const MAX_LEN: usize = 8;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_contacts<'a>(&'a mut self, out: &mut Vec<&'a mut Contact>) {
        for c in &mut self.live {
            if out.len() >= out.capacity() {
                break;
            }
            out.push(c);
        }
    }
}
