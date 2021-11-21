use std::fmt;

use crate::Entry;

use super::{dict::Dict, int::Int};

/// A bencode list
pub struct List<'b, 'p> {
    pub(crate) entry: Entry<'b, 'p>,
}

impl fmt::Debug for List<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<'b, 'p> IntoIterator for List<'b, 'p> {
    type Item = Entry<'b, 'p>;
    type IntoIter = ListIter<'b, 'p>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'b, 'p> List<'b, 'p> {
    /// Gets an iterator over the entries of the list
    pub fn iter(&self) -> ListIter<'b, 'p> {
        ListIter::new(self.entry)
    }

    /// Returns raw bytes of this list.
    ///
    /// # Examples
    ///
    /// Basic usage:
    /// ```
    /// use ben::{Parser, Entry};
    ///
    /// let bytes = b"l1:a1:be";
    /// let parser = &mut Parser::new();
    /// let dict = parser.parse::<Entry>(bytes).unwrap().as_list().unwrap();
    /// assert_eq!(b"l1:a1:be", dict.as_raw_bytes());
    /// ```
    pub fn as_raw_bytes(&self) -> &'b [u8] {
        self.entry.as_raw_bytes()
    }

    /// Returns the `Entry` at the given index.
    pub fn get(&self, i: usize) -> Option<Entry<'b, 'p>> {
        self.iter().nth(i)
    }

    /// Returns the `Dict` at the given index.
    pub fn get_dict(&self, i: usize) -> Option<Dict<'b, 'p>> {
        self.get(i)?.as_dict()
    }

    /// Returns the `List` at the given index.
    pub fn get_list(&self, i: usize) -> Option<List<'b, 'p>> {
        self.get(i)?.as_list()
    }

    /// Returns the byte slice at the given index.
    pub fn get_bytes(&self, i: usize) -> Option<&'b [u8]> {
        self.get(i)?.as_bytes()
    }

    /// Returns the string slice at the given index.
    pub fn get_str(&self, i: usize) -> Option<&'b str> {
        self.get(i)?.as_str()
    }

    /// Returns the printable ASCII string slice at the given index.
    pub fn get_ascii_str(&self, i: usize) -> Option<&'b str> {
        self.get(i)?.as_ascii_str()
    }

    /// Returns the `Int` at the given index.
    pub fn get_int<I>(&self, i: usize) -> Option<I>
    where
        I: Int,
    {
        self.get(i)?.as_int()
    }

    /// Returns true if the list is empty
    pub fn is_empty(&self) -> bool {
        self.entry.token().next == 1
    }
}

pub struct ListIter<'b, 'p> {
    entry: Entry<'b, 'p>,
    index: u32,
    end: u32,
}

impl<'b, 'p> ListIter<'b, 'p> {
    pub(crate) fn new(entry: Entry<'b, 'p>) -> Self {
        Self {
            index: 1,
            end: entry.token().next,
            entry,
        }
    }
}

impl<'b, 'p> Iterator for ListIter<'b, 'p> {
    type Item = Entry<'b, 'p>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.end {
            return None;
        }

        // Safety: Validated by the parser
        let token = unsafe { self.entry.token.add(self.index as usize) };
        let entry = Entry::from_raw(self.entry.buf, token);
        self.index += entry.token().next;

        Some(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::*;
    use crate::Error;

    #[test]
    fn list_get() {
        let s = b"ld1:alee1:be";
        let p = &mut Parser::new();
        let list = p.parse::<List>(s).unwrap();
        let n = list.get(1).unwrap();
        assert_eq!(b"b", n.as_raw_bytes());
    }

    #[test]
    fn list_get_nested() {
        let s = b"l1:ad1:al1:aee1:be";
        let p = &mut Parser::new();
        let list = p.parse::<List>(s).unwrap();
        assert_eq!(b"a", list.get(0).unwrap().as_raw_bytes());
        assert_eq!(b"d1:al1:aee", list.get(1).unwrap().as_raw_bytes());
        assert_eq!(b"b", list.get(2).unwrap().as_raw_bytes());
        assert_eq!(None, list.get(3));
    }

    #[test]
    fn list_get_overflow() {
        let s = b"l1:al1:ad1:al1:aee1:be1:be";
        let p = &mut Parser::new();
        let list = p.parse::<List>(s).unwrap();
        let list = list.get_list(1).unwrap();
        assert_eq!(b"a", list.get(0).unwrap().as_raw_bytes());
        assert_eq!(b"d1:al1:aee", list.get(1).unwrap().as_raw_bytes());
        assert_eq!(b"b", list.get(2).unwrap().as_raw_bytes());
        assert_eq!(None, list.get(3));
    }

    #[test]
    fn list_iter() {
        let s = b"l1:ad1:al1:aee1:be";
        let p = &mut Parser::new();
        let list = p.parse::<List>(s).unwrap();
        let mut iter = list.iter();
        assert_eq!(b"a", iter.next().unwrap().as_raw_bytes());
        assert_eq!(b"d1:al1:aee", iter.next().unwrap().as_raw_bytes());
        assert_eq!(b"b", iter.next().unwrap().as_raw_bytes());
        assert_eq!(None, iter.next());
    }

    #[test]
    fn list_iter_not_a_list() {
        let s = b"de";
        let p = &mut Parser::new();
        let err = p.parse::<List>(s).unwrap_err();
        assert_eq!(err, Error::Decode);
    }

    #[test]
    fn empty_list_len() {
        let s = b"le";
        let p = &mut Parser::new();
        let list = p.parse::<List>(s).unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn non_empty_list_len() {
        let s = b"l1:a1:be";
        let p = &mut Parser::new();
        let list = p.parse::<List>(s).unwrap();
        assert!(!list.is_empty());
        assert_eq!(list.get_str(0).unwrap(), "a");
        assert_eq!(list.get_str(1).unwrap(), "b");
    }

    #[test]
    fn non_empty_list_nested_len() {
        let s = b"l1:ad1:al1:aee1:be";
        let p = &mut Parser::new();
        let list = p.parse::<List>(s).unwrap();
        assert!(!list.is_empty());
        assert_eq!(list.get_str(0).unwrap(), "a");
        assert_eq!(list.get(1).unwrap().as_raw_bytes(), b"d1:al1:aee");
        assert_eq!(list.get_str(2).unwrap(), "b");
    }

    #[test]
    fn list_get_invalid() {
        let s = b"l1:a1:be";
        let p = &mut Parser::new();
        let list = p.parse::<List>(s).unwrap();
        assert!(list.get_dict(0).is_none());
        assert!(list.get_list(1).is_none());
    }
}
