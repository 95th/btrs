use std::fmt;

use super::{
    int::Int,
    list::{List, ListIter},
    Entry,
};

/// A bencode dictionary
pub struct Dict<'b, 'p> {
    pub(crate) entry: Entry<'b, 'p>,
}

impl fmt::Debug for Dict<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<'b, 'p> IntoIterator for Dict<'b, 'p> {
    type Item = (&'b str, Entry<'b, 'p>);
    type IntoIter = DictIter<'b, 'p>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'b, 'p> Dict<'b, 'p> {
    /// Gets an iterator over the entries of the dictionary.
    pub fn iter(&self) -> DictIter<'b, 'p> {
        DictIter::new(self.entry)
    }
    /// Returns raw bytes of this dictionary.
    ///
    /// # Examples
    ///
    /// Basic usage:
    /// ```
    /// use ben::{Parser, Entry};
    ///
    /// let bytes = b"d1:a1:be";
    /// let parser = &mut Parser::new();
    /// let dict = parser.parse::<Entry>(bytes).unwrap().as_dict().unwrap();
    /// assert_eq!(b"d1:a1:be", dict.as_raw_bytes());
    /// ```
    pub fn as_raw_bytes(&self) -> &'b [u8] {
        self.entry.as_raw_bytes()
    }

    /// Returns the `Entry` for the given key.
    pub fn get(&self, key: &str) -> Option<Entry<'b, 'p>> {
        self.iter()
            .find_map(|(k, v)| if k == key { Some(v) } else { None })
    }

    /// Returns the `Dict` for the given key.
    pub fn get_dict(&self, key: &str) -> Option<Dict<'b, 'p>> {
        self.get(key)?.as_dict()
    }

    /// Returns the `List` for the given key.
    pub fn get_list(&self, key: &str) -> Option<List<'b, 'p>> {
        self.get(key)?.as_list()
    }

    /// Returns the byte slice for the given key.
    pub fn get_bytes(&self, key: &str) -> Option<&'b [u8]> {
        self.get(key)?.as_bytes()
    }

    /// Returns the string slice for the given key.
    pub fn get_str(&self, key: &str) -> Option<&'b str> {
        self.get(key)?.as_str()
    }

    /// Returns the printable ASCII string slice for the given key.
    pub fn get_ascii_str(&self, key: &str) -> Option<&'b str> {
        self.get(key)?.as_ascii_str()
    }

    /// Returns the `Int` for the given key.
    pub fn get_int<I>(&self, key: &str) -> Option<I>
    where
        I: Int,
    {
        self.get(key)?.as_int()
    }

    /// Returns true if the dictionary is empty
    pub fn is_empty(&self) -> bool {
        self.entry.token().next == 1
    }
}

pub struct DictIter<'b, 'p> {
    iter: ListIter<'b, 'p>,
}

impl<'b, 'p> DictIter<'b, 'p> {
    fn new(entry: Entry<'b, 'p>) -> Self {
        Self {
            iter: ListIter::new(entry),
        }
    }
}

impl<'b, 'p> Iterator for DictIter<'b, 'p> {
    type Item = (&'b str, Entry<'b, 'p>);

    fn next(&mut self) -> Option<Self::Item> {
        let key = self.iter.next()?;
        let value = self.iter.next()?;

        // Safety: Validated by the parser
        let key = unsafe { std::str::from_utf8_unchecked(key.as_raw_bytes()) };

        Some((key, value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::*;

    #[test]
    fn dict_iter() {
        let s = b"d1:a2:bc3:def4:ghije";
        let p = &mut Parser::new();
        let dict = p.parse::<Dict>(s).unwrap();
        let mut iter = dict.iter();

        let (k, v) = iter.next().unwrap();
        assert_eq!("a", k);
        assert_eq!(b"bc", v.as_raw_bytes());

        let (k, v) = iter.next().unwrap();
        assert_eq!("def", k);
        assert_eq!(b"ghij", v.as_raw_bytes());

        assert_eq!(None, iter.next());
    }

    #[test]
    fn dict_iter_2() {
        let s = b"d1:alee";
        let p = &mut Parser::new();
        let dict = p.parse::<Dict>(s).unwrap();
        let mut iter = dict.iter();

        let (k, v) = iter.next().unwrap();
        assert_eq!("a", k);
        assert_eq!(b"le", v.as_raw_bytes());

        assert_eq!(None, iter.next());
    }

    #[test]
    fn dict_iter_inside_list() {
        let s = b"ld1:alee1:a1:ae";
        let p = &mut Parser::new();
        let list = p.parse::<List>(s).unwrap();
        let mut list_iter = list.iter();

        let dict = list_iter.next().unwrap();
        assert_eq!(b"a", list_iter.next().unwrap().as_raw_bytes());
        assert_eq!(b"a", list_iter.next().unwrap().as_raw_bytes());
        assert_eq!(None, list_iter.next());

        let mut iter = dict.as_dict().unwrap().iter();

        let (k, v) = iter.next().unwrap();
        assert_eq!("a", k);
        assert_eq!(b"le", v.as_raw_bytes());

        assert_eq!(None, iter.next());
    }

    #[test]
    fn dict_get() {
        let s = b"d1:ai1e1:bi2ee";
        let p = &mut Parser::new();
        let dict = p.parse::<Dict>(s).unwrap();
        let b = dict.get("b").unwrap();
        assert_eq!(2, b.as_int().unwrap());
    }

    #[test]
    fn dict_get_invalid() {
        let s = b"d1:ai1e1:bi2ee";
        let p = &mut Parser::new();
        let dict = p.parse::<Dict>(s).unwrap();
        assert!(dict.get_dict("b").is_none());
        assert!(dict.get_list("b").is_none());
    }

    #[test]
    fn empty_dict_len() {
        let s = b"de";
        let p = &mut Parser::new();
        let dict = p.parse::<Dict>(s).unwrap();
        assert!(dict.is_empty());
    }

    #[test]
    fn non_empty_dict_len() {
        let s = b"d1:a1:be";
        let p = &mut Parser::new();
        let dict = p.parse::<Dict>(s).unwrap();
        assert!(!dict.is_empty());
        assert_eq!(dict.get_str("a").unwrap(), "b");
    }

    #[test]
    fn non_empty_dict_nested_len() {
        let s = b"d1:al1:ad1:al1:aee1:bee";
        let p = &mut Parser::new();
        let dict = p.parse::<Dict>(s).unwrap();
        assert!(!dict.is_empty());
        assert_eq!(dict.get("a").unwrap().as_raw_bytes(), b"l1:ad1:al1:aee1:be");
    }
}
