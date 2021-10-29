use crate::error::{Error, Result};
use crate::token::{Token, TokenKind};
use std::fmt;
use std::marker::PhantomData;

/// Decode to given type using provided `Entry` object
pub trait Decode<'a>: Sized {
    fn decode(entry: Entry<'a>) -> Result<Self>;
}

impl<'a> Decode<'a> for Entry<'a> {
    fn decode(entry: Entry<'a>) -> Result<Self> {
        Ok(entry)
    }
}

impl<'a> Decode<'a> for List<'a> {
    fn decode(entry: Entry<'a>) -> Result<Self> {
        match entry.into_list() {
            Some(dict) => Ok(dict),
            None => Err(Error::TypeMismatch("Not a list")),
        }
    }
}

impl<'a> Decode<'a> for Dict<'a> {
    fn decode(entry: Entry<'a>) -> Result<Self> {
        match entry.into_dict() {
            Some(dict) => Ok(dict),
            None => Err(Error::TypeMismatch("Not a dictionary")),
        }
    }
}

impl<'a> Decode<'a> for &'a [u8] {
    fn decode(entry: Entry<'a>) -> Result<Self> {
        match entry.as_bytes() {
            Some(val) => Ok(val),
            None => Err(Error::TypeMismatch("Not a byte string")),
        }
    }
}

impl<'a> Decode<'a> for Vec<u8> {
    fn decode(entry: Entry<'a>) -> Result<Self> {
        match entry.as_bytes() {
            Some(val) => Ok(val.to_vec()),
            None => Err(Error::TypeMismatch("Not a byte string")),
        }
    }
}

impl<'a> Decode<'a> for &'a str {
    fn decode(entry: Entry<'a>) -> Result<Self> {
        match entry.as_str() {
            Some(val) => Ok(val),
            None => Err(Error::TypeMismatch("Not a UTF-8 string")),
        }
    }
}

impl<'a> Decode<'a> for i64 {
    fn decode(entry: Entry<'a>) -> Result<Self> {
        match entry.as_int() {
            Some(val) => Ok(val),
            None => Err(Error::TypeMismatch("Not a integer")),
        }
    }
}

impl<'a> Decode<'a> for String {
    fn decode(entry: Entry<'a>) -> Result<Self> {
        match entry.as_str() {
            Some(val) => Ok(String::from(val)),
            None => Err(Error::TypeMismatch("Not a UTF-8 string")),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub struct Entry<'a> {
    pub(crate) buf: *const u8,
    pub(crate) token: *const Token,
    _marker: PhantomData<&'a ()>,
}

impl fmt::Debug for Entry<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.token().kind {
            TokenKind::Int => write!(f, "{}", self.as_int().unwrap()),
            TokenKind::ByteStr => match self.as_ascii_str() {
                Some(s) => write!(f, "\"{}\"", s),
                None => write!(f, "'{}'", data_encoding::BASE32.encode(self.as_raw_bytes())),
            },
            TokenKind::List => self.into_list().unwrap().fmt(f),
            TokenKind::Dict => self.into_dict().unwrap().fmt(f),
        }
    }
}

impl<'a> Entry<'a> {
    pub(crate) fn new(buf: &'a [u8], tokens: &'a [Token]) -> Self {
        Entry::from_raw(buf.as_ptr(), tokens.as_ptr())
    }

    fn from_raw(buf: *const u8, token: *const Token) -> Self {
        Entry {
            buf,
            token,
            _marker: PhantomData,
        }
    }

    fn token(&self) -> &Token {
        // Safety: Validated by the parser
        unsafe { &*self.token }
    }

    /// Returns raw bytes of this entry.
    ///
    /// This returns complete raw bytes for dict and list, but remove the headers
    /// from string and int.
    ///
    /// # Examples
    ///
    /// Basic usage:
    /// ```
    /// use ben::{Parser, Entry};
    ///
    /// let bytes = b"l1:a2:bce";
    /// let parser = &mut Parser::new();
    /// let entry = parser.parse::<Entry>(bytes).unwrap();
    /// assert_eq!(b"l1:a2:bce", entry.as_raw_bytes());
    /// ```
    pub fn as_raw_bytes(&self) -> &'a [u8] {
        // Safety: Tokens are always in-bounds (ensured by parser)
        unsafe {
            let t = self.token();
            let p = self.buf.add(t.start as usize);
            std::slice::from_raw_parts(p, t.len as usize)
        }
    }

    /// Returns true if this entry is a list.
    pub fn is_list(&self) -> bool {
        self.token().kind == TokenKind::List
    }

    /// Returns true if this entry is a dictionary.
    pub fn is_dict(&self) -> bool {
        self.token().kind == TokenKind::Dict
    }

    /// Returns true if this entry is a string.
    pub fn is_bytes(&self) -> bool {
        self.token().kind == TokenKind::ByteStr
    }

    /// Returns true if this entry is a integer.
    pub fn is_int(&self) -> bool {
        self.token().kind == TokenKind::Int
    }

    /// Return this entry as a `List` which provides further
    /// list operations such as `get`, `iter` etc.
    ///
    /// # Examples
    ///
    /// Basic usage:
    /// ```
    /// use ben::{Parser, Entry};
    ///
    /// let bytes = b"l1:a2:bce";
    /// let parser = &mut Parser::new();
    /// let entry = parser.parse::<Entry>(bytes).unwrap();
    /// let list = entry.into_list().unwrap();
    /// assert_eq!(b"a", list.get_bytes(0).unwrap());
    /// assert_eq!(b"bc", list.get_bytes(1).unwrap());
    /// ```
    pub fn into_list(self) -> Option<List<'a>> {
        if self.is_list() {
            Some(List { entry: self })
        } else {
            None
        }
    }

    /// Return this entry as a `Dict` which provides further
    /// dictionary operations such as `get`, `iter` etc.
    ///
    /// # Examples
    ///
    /// Basic usage:
    /// ```
    /// use ben::{Parser, Entry};
    ///
    /// let bytes = b"d1:a2:bce";
    /// let parser = &mut Parser::new();
    /// let entry = parser.parse::<Entry>(bytes).unwrap();
    /// let dict = entry.into_dict().unwrap();
    /// assert_eq!(b"bc", dict.get_bytes("a").unwrap());
    /// ```
    pub fn into_dict(self) -> Option<Dict<'a>> {
        if self.is_dict() {
            Some(Dict { entry: self })
        } else {
            None
        }
    }

    /// Return this entry as a `i64`.
    ///
    /// # Examples
    ///
    /// Basic usage:
    /// ```
    /// use ben::{Parser, Entry};
    ///
    /// let bytes = b"i123e";
    /// let parser = &mut Parser::new();
    /// let entry = parser.parse::<Entry>(bytes).unwrap();
    /// assert_eq!(123, entry.as_int().unwrap());
    /// ```
    pub fn as_int(&self) -> Option<i64> {
        if !self.is_int() {
            return None;
        }
        let mut val = 0;
        let mut negative = false;
        for &c in self.as_raw_bytes() {
            if c == b'-' {
                negative = true;
            } else {
                let digit = i64::from(c - b'0');
                val = (val * 10) + digit;
            }
        }
        if negative {
            val *= -1;
        }
        Some(val)
    }

    /// Return this entry as a byte slice.
    ///
    /// # Examples
    ///
    /// Basic usage:
    /// ```
    /// use ben::{Parser, Entry};
    ///
    /// let bytes = b"3:abc";
    /// let parser = &mut Parser::new();
    /// let entry = parser.parse::<Entry>(bytes).unwrap();
    /// assert_eq!(b"abc", entry.as_bytes().unwrap());
    /// ```
    pub fn as_bytes(&self) -> Option<&'a [u8]> {
        if self.is_bytes() {
            Some(self.as_raw_bytes())
        } else {
            None
        }
    }

    /// Return this entry as a string slice.
    ///
    /// Returns None if this entry is not a valid UTF-8 byte string
    ///
    /// # Examples
    ///
    /// Basic usage:
    /// ```
    /// use ben::{Parser, Entry};
    ///
    /// let bytes = b"3:abc";
    /// let parser = &mut Parser::new();
    /// let entry = parser.parse::<Entry>(bytes).unwrap();
    /// assert_eq!("abc", entry.as_str().unwrap());
    /// ```
    pub fn as_str(&self) -> Option<&'a str> {
        let bytes = self.as_bytes()?;
        std::str::from_utf8(bytes).ok()
    }

    /// Return this entry as a string slice.
    ///
    /// Returns None if this entry
    /// 1. is not a valid UTF-8 string.
    /// 2. contains characters except ASCII alphanumeric, punctuation and whitespace.
    ///
    /// # Examples
    ///
    /// Basic usage:
    /// ```
    /// use ben::{Parser, Entry};
    ///
    /// let parser = &mut Parser::new();
    /// let entry = parser.parse::<Entry>(b"3:abc").unwrap();
    /// assert_eq!("abc", entry.as_ascii_str().unwrap());
    ///
    /// let entry = parser.parse::<Entry>(b"3:\x01\x01\x01").unwrap();
    /// assert!(entry.as_ascii_str().is_none());
    /// ```
    pub fn as_ascii_str(&self) -> Option<&'a str> {
        let s = self.as_str()?;
        let is_ascii = |c: char| {
            c.is_ascii_alphanumeric() || c.is_ascii_punctuation() || c.is_ascii_whitespace()
        };
        if s.chars().all(is_ascii) {
            Some(s)
        } else {
            None
        }
    }
}

/// A bencode list
pub struct List<'a> {
    entry: Entry<'a>,
}

impl fmt::Debug for List<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<'a> IntoIterator for List<'a> {
    type Item = Entry<'a>;
    type IntoIter = ListIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a> List<'a> {
    /// Gets an iterator over the entries of the list
    pub fn iter(&self) -> ListIter<'a> {
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
    /// let dict = parser.parse::<Entry>(bytes).unwrap().into_list().unwrap();
    /// assert_eq!(b"l1:a1:be", dict.as_raw_bytes());
    /// ```
    pub fn as_raw_bytes(&self) -> &'a [u8] {
        self.entry.as_raw_bytes()
    }

    /// Returns the `Entry` at the given index.
    pub fn get(&self, i: usize) -> Option<Entry<'a>> {
        self.iter().nth(i)
    }

    /// Returns the `Dict` at the given index.
    pub fn get_dict(&self, i: usize) -> Option<Dict<'a>> {
        self.get(i)?.into_dict()
    }

    /// Returns the `List` at the given index.
    pub fn get_list(&self, i: usize) -> Option<List<'a>> {
        self.get(i)?.into_list()
    }

    /// Returns the byte slice at the given index.
    pub fn get_bytes(&self, i: usize) -> Option<&'a [u8]> {
        self.get(i)?.as_bytes()
    }

    /// Returns the string slice at the given index.
    pub fn get_str(&self, i: usize) -> Option<&'a str> {
        self.get(i)?.as_str()
    }

    /// Returns the printable ASCII string slice at the given index.
    pub fn get_ascii_str(&self, i: usize) -> Option<&'a str> {
        self.get(i)?.as_ascii_str()
    }

    /// Returns the `i64` at the given index.
    pub fn get_int(&self, i: usize) -> Option<i64> {
        self.get(i)?.as_int()
    }

    /// Returns true if the list is empty
    pub fn is_empty(&self) -> bool {
        self.entry.token().next == 1
    }
}

pub struct ListIter<'a> {
    entry: Entry<'a>,
    index: usize,
    end: usize,
}

impl<'a> ListIter<'a> {
    fn new(entry: Entry<'a>) -> Self {
        Self {
            index: 1,
            end: entry.token().next as usize,
            entry,
        }
    }
}

impl<'a> Iterator for ListIter<'a> {
    type Item = Entry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.end {
            return None;
        }

        // Safety: Validated by the parser
        let token = unsafe { self.entry.token.add(self.index) };
        let entry = Entry::from_raw(self.entry.buf, token);
        self.index += entry.token().next as usize;

        Some(entry)
    }
}

/// A bencode dictionary
pub struct Dict<'a> {
    entry: Entry<'a>,
}

impl fmt::Debug for Dict<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<'a> IntoIterator for Dict<'a> {
    type Item = (&'a str, Entry<'a>);
    type IntoIter = DictIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a> Dict<'a> {
    /// Gets an iterator over the entries of the dictionary.
    pub fn iter(&self) -> DictIter<'a> {
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
    /// let dict = parser.parse::<Entry>(bytes).unwrap().into_dict().unwrap();
    /// assert_eq!(b"d1:a1:be", dict.as_raw_bytes());
    /// ```
    pub fn as_raw_bytes(&self) -> &'a [u8] {
        self.entry.as_raw_bytes()
    }

    /// Returns the `Entry` for the given key.
    pub fn get(&self, key: &str) -> Option<Entry<'a>> {
        self.iter()
            .find_map(|(k, v)| if k == key { Some(v) } else { None })
    }

    /// Returns the `Dict` for the given key.
    pub fn get_dict(&self, key: &str) -> Option<Dict<'a>> {
        self.get(key)?.into_dict()
    }

    /// Returns the `List` for the given key.
    pub fn get_list(&self, key: &str) -> Option<List<'a>> {
        self.get(key)?.into_list()
    }

    /// Returns the byte slice for the given key.
    pub fn get_bytes(&self, key: &str) -> Option<&'a [u8]> {
        self.get(key)?.as_bytes()
    }

    /// Returns the string slice for the given key.
    pub fn get_str(&self, key: &str) -> Option<&'a str> {
        self.get(key)?.as_str()
    }

    /// Returns the printable ASCII string slice for the given key.
    pub fn get_ascii_str(&self, key: &str) -> Option<&'a str> {
        self.get(key)?.as_ascii_str()
    }

    /// Returns the `i64` for the given key.
    pub fn get_int(&self, key: &str) -> Option<i64> {
        self.get(key)?.as_int()
    }

    /// Returns true if the dictionary is empty
    pub fn is_empty(&self) -> bool {
        self.entry.token().next == 1
    }
}

pub struct DictIter<'a> {
    iter: ListIter<'a>,
}

impl<'a> DictIter<'a> {
    fn new(entry: Entry<'a>) -> Self {
        Self {
            iter: ListIter::new(entry),
        }
    }
}

impl<'a> Iterator for DictIter<'a> {
    type Item = (&'a str, Entry<'a>);

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
    use crate::Error;

    #[test]
    fn list_get() {
        let s = b"ld1:alee1:be";
        let parser = &mut Parser::new();
        let list = parser.parse::<List>(s).unwrap();
        let n = list.get(1).unwrap();
        assert_eq!(b"b", n.as_raw_bytes());
    }

    #[test]
    fn list_get_nested() {
        let s = b"l1:ad1:al1:aee1:be";
        let parser = &mut Parser::new();
        let list = parser.parse::<List>(s).unwrap();
        assert_eq!(b"a", list.get(0).unwrap().as_raw_bytes());
        assert_eq!(b"d1:al1:aee", list.get(1).unwrap().as_raw_bytes());
        assert_eq!(b"b", list.get(2).unwrap().as_raw_bytes());
        assert_eq!(None, list.get(3));
    }

    #[test]
    fn list_get_overflow() {
        let s = b"l1:al1:ad1:al1:aee1:be1:be";
        let parser = &mut Parser::new();
        let list = parser.parse::<List>(s).unwrap();
        let list = list.get_list(1).unwrap();
        assert_eq!(b"a", list.get(0).unwrap().as_raw_bytes());
        assert_eq!(b"d1:al1:aee", list.get(1).unwrap().as_raw_bytes());
        assert_eq!(b"b", list.get(2).unwrap().as_raw_bytes());
        assert_eq!(None, list.get(3));
    }

    #[test]
    fn list_iter() {
        let s = b"l1:ad1:al1:aee1:be";
        let parser = &mut Parser::new();
        let list = parser.parse::<List>(s).unwrap();
        let mut iter = list.iter();
        assert_eq!(b"a", iter.next().unwrap().as_raw_bytes());
        assert_eq!(b"d1:al1:aee", iter.next().unwrap().as_raw_bytes());
        assert_eq!(b"b", iter.next().unwrap().as_raw_bytes());
        assert_eq!(None, iter.next());
    }

    #[test]
    fn list_iter_not_a_list() {
        let s = b"de";
        let parser = &mut Parser::new();
        let err = parser.parse::<List>(s).unwrap_err();
        assert_eq!(err, Error::TypeMismatch("Not a list"));
    }

    #[test]
    fn dict_iter() {
        let s = b"d1:a2:bc3:def4:ghije";
        let parser = &mut Parser::new();
        let dict = parser.parse::<Dict>(s).unwrap();
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
        let parser = &mut Parser::new();
        let dict = parser.parse::<Dict>(s).unwrap();
        let mut iter = dict.iter();

        let (k, v) = iter.next().unwrap();
        assert_eq!("a", k);
        assert_eq!(b"le", v.as_raw_bytes());

        assert_eq!(None, iter.next());
    }

    #[test]
    fn dict_iter_inside_list() {
        let s = b"ld1:alee1:a1:ae";
        let parser = &mut Parser::new();
        let list = parser.parse::<List>(s).unwrap();
        let mut list_iter = list.iter();

        let dict = list_iter.next().unwrap();
        assert_eq!(b"a", list_iter.next().unwrap().as_raw_bytes());
        assert_eq!(b"a", list_iter.next().unwrap().as_raw_bytes());
        assert_eq!(None, list_iter.next());

        let mut iter = dict.into_dict().unwrap().iter();

        let (k, v) = iter.next().unwrap();
        assert_eq!("a", k);
        assert_eq!(b"le", v.as_raw_bytes());

        assert_eq!(None, iter.next());
    }

    #[test]
    fn int_value() {
        let s = b"i12e";
        let parser = &mut Parser::new();
        let n = parser.parse::<i64>(s).unwrap();
        assert_eq!(12, n);
    }

    #[test]
    fn int_value_negative() {
        let s = b"i-12e";
        let parser = &mut Parser::new();
        let n = parser.parse::<i64>(s).unwrap();
        assert_eq!(-12, n);
    }

    #[test]
    fn int_value_invalid() {
        let s = b"ixyze";
        let err = Parser::new().parse::<i64>(s).unwrap_err();
        assert_eq!(Error::Unexpected { pos: 1 }, err);
    }

    #[test]
    fn str_value() {
        let s = b"5:abcde";
        let parser = &mut Parser::new();
        let bytes = parser.parse::<&[u8]>(s).unwrap();
        assert_eq!(b"abcde", bytes);
    }

    #[test]
    fn dict_get() {
        let s = b"d1:ai1e1:bi2ee";
        let parser = &mut Parser::new();
        let dict = parser.parse::<Dict>(s).unwrap();
        let b = dict.get("b").unwrap();
        assert_eq!(2, b.as_int().unwrap());
    }

    #[test]
    fn dict_get_invalid() {
        let s = b"d1:ai1e1:bi2ee";
        let parser = &mut Parser::new();
        let dict = parser.parse::<Dict>(s).unwrap();
        assert!(dict.get_dict("b").is_none());
        assert!(dict.get_list("b").is_none());
    }

    #[test]
    fn list_get_invalid() {
        let s = b"l1:a1:be";
        let parser = &mut Parser::new();
        let list = parser.parse::<List>(s).unwrap();
        assert!(list.get_dict(0).is_none());
        assert!(list.get_list(1).is_none());
    }

    #[test]
    fn decode_empty() {
        let parser = &mut Parser::new();
        let err = parser.parse::<Entry>(&[]).unwrap_err();
        assert_eq!(err, Error::Eof);
    }

    #[test]
    fn decode_debug_bytes() {
        let s = b"3:\x01\x01\x01";
        let parser = &mut Parser::new();
        let n = parser.parse::<Entry>(s).unwrap();
        assert!(n.as_bytes().is_some());
        assert!(n.as_ascii_str().is_none());
        assert_eq!(
            format!("'{}'", data_encoding::BASE32.encode(&[1, 1, 1])),
            format!("{:?}", n)
        );
    }

    #[test]
    fn decode_debug_str() {
        let s = b"3:abc";
        let parser = &mut Parser::new();
        let n = parser.parse::<Entry>(s).unwrap();
        assert!(n.as_bytes().is_some());
        assert!(n.as_ascii_str().is_some());
        assert_eq!("\"abc\"", format!("{:?}", n));
    }

    #[test]
    fn empty_dict_len() {
        let s = b"de";
        let parser = &mut Parser::new();
        let dict = parser.parse::<Dict>(s).unwrap();
        assert!(dict.is_empty());
    }

    #[test]
    fn non_empty_dict_len() {
        let s = b"d1:a1:be";
        let parser = &mut Parser::new();
        let dict = parser.parse::<Dict>(s).unwrap();
        assert!(!dict.is_empty());
        assert_eq!(dict.get_str("a").unwrap(), "b");
    }

    #[test]
    fn non_empty_dict_nested_len() {
        let s = b"d1:al1:ad1:al1:aee1:bee";
        let parser = &mut Parser::new();
        let dict = parser.parse::<Dict>(s).unwrap();
        assert!(!dict.is_empty());
        assert_eq!(dict.get("a").unwrap().as_raw_bytes(), b"l1:ad1:al1:aee1:be");
    }
    #[test]
    fn empty_list_len() {
        let s = b"le";
        let parser = &mut Parser::new();
        let list = parser.parse::<List>(s).unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn non_empty_list_len() {
        let s = b"l1:a1:be";
        let parser = &mut Parser::new();
        let list = parser.parse::<List>(s).unwrap();
        assert!(!list.is_empty());
        assert_eq!(list.get_str(0).unwrap(), "a");
        assert_eq!(list.get_str(1).unwrap(), "b");
    }

    #[test]
    fn non_empty_list_nested_len() {
        let s = b"l1:ad1:al1:aee1:be";
        let parser = &mut Parser::new();
        let list = parser.parse::<List>(s).unwrap();
        assert!(!list.is_empty());
        assert_eq!(list.get_str(0).unwrap(), "a");
        assert_eq!(list.get(1).unwrap().as_raw_bytes(), b"d1:al1:aee");
        assert_eq!(list.get_str(2).unwrap(), "b");
    }
}
