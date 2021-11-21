mod dict;
mod entry;
mod int;
mod list;

pub use dict::*;
pub use entry::Entry;
pub use int::*;
pub use list::*;

/// Decode to given type using provided `Entry` object
pub trait Decode<'b, 'p>: Sized {
    fn decode(entry: Entry<'b, 'p>) -> Option<Self>;
}

impl<'b, 'p> Decode<'b, 'p> for Entry<'b, 'p> {
    fn decode(entry: Entry<'b, 'p>) -> Option<Self> {
        Some(entry)
    }
}

impl<'b, 'p> Decode<'b, 'p> for List<'b, 'p> {
    fn decode(entry: Entry<'b, 'p>) -> Option<Self> {
        entry.as_list()
    }
}

impl<'b, 'p> Decode<'b, 'p> for Dict<'b, 'p> {
    fn decode(entry: Entry<'b, 'p>) -> Option<Self> {
        entry.as_dict()
    }
}

impl<'b, 'p> Decode<'b, 'p> for &'b [u8] {
    fn decode(entry: Entry<'b, 'p>) -> Option<Self> {
        entry.as_bytes()
    }
}

impl<'b, 'p> Decode<'b, 'p> for Vec<u8> {
    fn decode(entry: Entry<'b, 'p>) -> Option<Self> {
        entry.as_bytes().map(<[u8]>::to_vec)
    }
}

impl<'b, 'p> Decode<'b, 'p> for &'b str {
    fn decode(entry: Entry<'b, 'p>) -> Option<Self> {
        entry.as_str()
    }
}

impl<'b, 'p, I: Int> Decode<'b, 'p> for I {
    fn decode(entry: Entry<'b, 'p>) -> Option<Self> {
        entry.as_int()
    }
}

impl<'b, 'p> Decode<'b, 'p> for String {
    fn decode(entry: Entry<'b, 'p>) -> Option<Self> {
        entry.as_str().map(String::from)
    }
}
