use crate::token::{Token, TokenKind};
use std::fmt;

#[derive(PartialEq)]
#[repr(C)]
pub struct Node<'a> {
    pub(crate) buf: &'a [u8],
    pub(crate) token: &'a Token,
    pub(crate) rest: &'a [Token],
}

impl fmt::Debug for Node<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.token.kind {
            TokenKind::Int => write!(f, "{}", self.as_int().unwrap()),
            TokenKind::ByteStr => match self.as_ascii_str() {
                Some(s) => write!(f, "\"{}\"", s),
                None => write!(f, "`Bytes:{:?}`", self.as_raw_bytes()),
            },
            TokenKind::List => self.as_list().unwrap().fmt(f),
            TokenKind::Dict => self.as_dict().unwrap().fmt(f),
        }
    }
}

impl<'a> Node<'a> {
    pub(crate) fn new(buf: &'a [u8], tokens: &'a [Token]) -> Option<Self> {
        if let [token, rest @ ..] = tokens {
            Some(Node { buf, token, rest })
        } else {
            None
        }
    }

    /// Returns raw bytes of this node.
    ///
    /// This returns complete raw bytes for dict and list, but remove the headers
    /// from string and int.
    ///
    /// # Examples
    ///
    /// Basic usage:
    /// ```
    /// use ben::{Node, Parser};
    ///
    /// let bytes = b"l1:a2:bce";
    /// let parser = &mut Parser::new();
    /// let node = parser.parse(bytes).unwrap();
    /// assert_eq!(b"l1:a2:bce", node.as_raw_bytes());
    /// ```
    pub fn as_raw_bytes(&self) -> &'a [u8] {
        let buf = self.buf.get(self.token.range());
        debug_assert!(buf.is_some());
        buf.unwrap_or_default()
    }

    /// Returns true if this node is a list.
    pub fn is_list(&self) -> bool {
        self.token.kind == TokenKind::List
    }

    /// Returns true if this node is a dictionary.
    pub fn is_dict(&self) -> bool {
        self.token.kind == TokenKind::Dict
    }

    /// Returns true if this node is a string.
    pub fn is_bytes(&self) -> bool {
        self.token.kind == TokenKind::ByteStr
    }

    /// Returns true if this node is a integer.
    pub fn is_int(&self) -> bool {
        self.token.kind == TokenKind::Int
    }

    /// Return this node as a `List` which provides further
    /// list operations such as `get`, `iter` etc.
    ///
    /// # Examples
    ///
    /// Basic usage:
    /// ```
    /// use ben::{Node, Parser};
    ///
    /// let bytes = b"l1:a2:bce";
    /// let parser = &mut Parser::new();
    /// let node = parser.parse(bytes).unwrap();
    /// let list = node.to_list().unwrap();
    /// assert_eq!(b"a", list.get_bytes(0).unwrap());
    /// assert_eq!(b"bc", list.get_bytes(1).unwrap());
    /// ```
    pub fn to_list(self) -> Option<List<'a>> {
        if self.is_list() {
            Some(List {
                buf: self.buf,
                token: self.token,
                rest: self.rest,
            })
        } else {
            None
        }
    }

    /// Return this node as a `List` which provides further
    /// list operations such as `get`, `iter` etc.
    ///
    /// # Examples
    ///
    /// Basic usage:
    /// ```
    /// use ben::{Node, Parser};
    ///
    /// let bytes = b"l1:a2:bce";
    /// let parser = &mut Parser::new();
    /// let node = parser.parse(bytes).unwrap();
    /// let list = node.as_list().unwrap();
    /// assert_eq!(b"a", list.get_bytes(0).unwrap());
    /// assert_eq!(b"bc", list.get_bytes(1).unwrap());
    /// ```
    pub fn as_list(&self) -> Option<&List<'a>> {
        if self.is_list() {
            let list = unsafe { std::mem::transmute(self) };
            Some(list)
        } else {
            None
        }
    }

    /// Return this node as a `Dict` which provides further
    /// dictionary operations such as `get`, `iter` etc.
    ///
    /// # Examples
    ///
    /// Basic usage:
    /// ```
    /// use ben::{Node, Parser};
    ///
    /// let bytes = b"d1:a2:bce";
    /// let parser = &mut Parser::new();
    /// let node = parser.parse(bytes).unwrap();
    /// let dict = node.to_dict().unwrap();
    /// assert_eq!(b"bc", dict.get_bytes(b"a").unwrap());
    /// ```
    pub fn to_dict(self) -> Option<Dict<'a>> {
        if self.is_dict() {
            Some(Dict {
                buf: self.buf,
                token: self.token,
                rest: self.rest,
            })
        } else {
            None
        }
    }

    /// Return this node as a `Dict` which provides further
    /// dictionary operations such as `get`, `iter` etc.
    ///
    /// # Examples
    ///
    /// Basic usage:
    /// ```
    /// use ben::{Node, Parser};
    ///
    /// let bytes = b"d1:a2:bce";
    /// let parser = &mut Parser::new();
    /// let node = parser.parse(bytes).unwrap();
    /// let dict = node.as_dict().unwrap();
    /// assert_eq!(b"bc", dict.get_bytes(b"a").unwrap());
    /// ```
    pub fn as_dict(&self) -> Option<&Dict<'a>> {
        if self.is_dict() {
            let dict = unsafe { std::mem::transmute(self) };
            Some(dict)
        } else {
            None
        }
    }

    /// Return this node as a `i64`.
    ///
    /// # Examples
    ///
    /// Basic usage:
    /// ```
    /// use ben::{Node, Parser};
    ///
    /// let bytes = b"i123e";
    /// let parser = &mut Parser::new();
    /// let node = parser.parse(bytes).unwrap();
    /// assert_eq!(123, node.as_int().unwrap());
    /// ```
    pub fn as_int(&self) -> Option<i64> {
        if self.token.kind != TokenKind::Int {
            return None;
        }
        let mut val = 0;
        let mut negative = false;
        let bytes = self.buf.get(self.token.range());
        debug_assert!(bytes.is_some());
        for &c in bytes? {
            if c == b'-' {
                negative = true;
            } else {
                let digit = (c - b'0') as i64;
                val = (val * 10) + digit;
            }
        }
        if negative {
            val *= -1
        };
        Some(val)
    }

    /// Return this node as a byte slice.
    ///
    /// # Examples
    ///
    /// Basic usage:
    /// ```
    /// use ben::{Node, Parser};
    ///
    /// let bytes = b"3:abc";
    /// let parser = &mut Parser::new();
    /// let node = parser.parse(bytes).unwrap();
    /// assert_eq!(b"abc", node.as_bytes().unwrap());
    /// ```
    pub fn as_bytes(&self) -> Option<&'a [u8]> {
        if let TokenKind::ByteStr = self.token.kind {
            let bytes = self.buf.get(self.token.range());
            debug_assert!(bytes.is_some());
            bytes
        } else {
            None
        }
    }

    /// Return this node as a string slice.
    ///
    /// Returns None if this node is not a valid UTF-8 byte string
    ///
    /// # Examples
    ///
    /// Basic usage:
    /// ```
    /// use ben::{Node, Parser};
    ///
    /// let bytes = b"3:abc";
    /// let parser = &mut Parser::new();
    /// let node = parser.parse(bytes).unwrap();
    /// assert_eq!("abc", node.as_str().unwrap());
    /// ```
    pub fn as_str(&self) -> Option<&'a str> {
        let bytes = self.as_bytes()?;
        std::str::from_utf8(bytes).ok()
    }

    /// Return this node as a string slice.
    ///
    /// Returns None if this node
    /// 1. is not a valid UTF-8 string.
    /// 2. contains characters except ASCII alphanumeric, punctuation and whitespace.
    ///
    /// # Examples
    ///
    /// Basic usage:
    /// ```
    /// use ben::{Node, Parser};
    ///
    /// let parser = &mut Parser::new();
    /// let node = parser.parse(b"3:abc").unwrap();
    /// assert_eq!("abc", node.as_ascii_str().unwrap());
    ///
    /// let node = parser.parse(b"3:\x01\x01\x01").unwrap();
    /// assert!(node.as_ascii_str().is_none());
    /// ```
    pub fn as_ascii_str(&self) -> Option<&'a str> {
        let s = self.as_str()?;
        if s.chars().all(|c| {
            c.is_ascii_alphanumeric() || c.is_ascii_punctuation() || c.is_ascii_whitespace()
        }) {
            Some(s)
        } else {
            None
        }
    }
}

/// A bencode list
#[repr(C)]
pub struct List<'a> {
    buf: &'a [u8],
    token: &'a Token,
    rest: &'a [Token],
}

impl fmt::Debug for List<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<'a> List<'a> {
    /// Gets an iterator over the entries of the list
    pub fn iter(&self) -> ListIter<'a> {
        ListIter {
            buf: self.buf,
            tokens: self.rest,
            total: self.len(),
            idx: 0,
            pos: 0,
        }
    }

    /// Returns the `Node` at the given index.
    pub fn get(&self, i: usize) -> Option<Node<'a>> {
        let idx = self.find_idx(i)?;
        let tokens = self.rest.get(idx..)?;
        Node::new(self.buf, tokens)
    }

    /// Returns the `Dict` at the given index.
    pub fn get_dict(&self, i: usize) -> Option<Dict<'a>> {
        self.get(i)?.to_dict()
    }

    /// Returns the `List` at the given index.
    pub fn get_list(&self, i: usize) -> Option<List<'a>> {
        self.get(i)?.to_list()
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

    /// Returns the number of items
    pub fn len(&self) -> usize {
        self.token.children as usize
    }

    /// Returns true if the list is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Find the index of i'th element in the tokens array
    fn find_idx(&self, i: usize) -> Option<usize> {
        if i >= self.len() {
            return None;
        }
        let mut idx = 0;
        let mut item = 0;

        while item < i {
            debug_assert!(idx < self.rest.len());
            idx += self.rest.get(idx)?.next as usize;
            item += 1;
        }

        Some(idx)
    }
}

pub struct ListIter<'a> {
    buf: &'a [u8],
    tokens: &'a [Token],
    total: usize,
    idx: usize,
    pos: usize,
}

impl<'a> Iterator for ListIter<'a> {
    type Item = Node<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.total {
            return None;
        }

        debug_assert!(self.idx < self.tokens.len());
        let tokens = self.tokens.get(self.idx..)?;
        let node = Node::new(self.buf, tokens)?;

        self.idx += node.token.next as usize;
        self.pos += 1;

        Some(node)
    }
}

/// A bencode dictionary
#[repr(C)]
pub struct Dict<'a> {
    buf: &'a [u8],
    token: &'a Token,
    rest: &'a [Token],
}

impl fmt::Debug for Dict<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<'a> Dict<'a> {
    /// Gets an iterator over the entries of the dictionary.
    pub fn iter(&self) -> DictIter<'a> {
        DictIter {
            buf: self.buf,
            tokens: self.rest,
            total: self.len(),
            idx: 0,
            pos: 0,
        }
    }

    /// Returns the `Node` for the given key.
    pub fn get(&self, key: &[u8]) -> Option<Node<'a>> {
        self.iter()
            .find(|(k, _)| k.as_bytes() == Some(key))
            .map(|(_, v)| v)
    }

    /// Returns the `Dict` for the given key.
    pub fn get_dict(&self, key: &[u8]) -> Option<Dict<'a>> {
        self.get(key)?.to_dict()
    }

    /// Returns the `List` for the given key.
    pub fn get_list(&self, key: &[u8]) -> Option<List<'a>> {
        self.get(key)?.to_list()
    }

    /// Returns the byte slice for the given key.
    pub fn get_bytes(&self, key: &[u8]) -> Option<&'a [u8]> {
        self.get(key)?.as_bytes()
    }

    /// Returns the string slice for the given key.
    pub fn get_str(&self, key: &[u8]) -> Option<&'a str> {
        self.get(key)?.as_str()
    }

    /// Returns the printable ASCII string slice for the given key.
    pub fn get_ascii_str(&self, key: &[u8]) -> Option<&'a str> {
        self.get(key)?.as_ascii_str()
    }

    /// Returns the `i64` for the given key.
    pub fn get_int(&self, key: &[u8]) -> Option<i64> {
        self.get(key)?.as_int()
    }

    /// Returns the number of entries
    pub fn len(&self) -> usize {
        debug_assert_eq!(self.token.children % 2, 0);
        self.token.children as usize / 2
    }

    /// Returns true if the dictionary is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub struct DictIter<'a> {
    buf: &'a [u8],
    tokens: &'a [Token],
    total: usize,
    idx: usize,
    pos: usize,
}

impl<'a> Iterator for DictIter<'a> {
    type Item = (Node<'a>, Node<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.total {
            return None;
        }

        debug_assert!(self.idx < self.tokens.len());
        let tokens = self.tokens.get(self.idx..)?;
        let key = Node::new(self.buf, tokens)?;

        debug_assert_eq!(TokenKind::ByteStr, key.token.kind);
        self.idx += key.token.next as usize;

        debug_assert!(self.idx < self.tokens.len());
        let tokens = self.tokens.get(self.idx..)?;
        let val = Node::new(self.buf, tokens)?;

        self.idx += val.token.next as usize;
        self.pos += 1;

        Some((key, val))
    }
}

#[cfg(test)]
mod tests {
    use crate::parse::*;
    use crate::Error;

    #[test]
    fn list_get() {
        let s = b"ld1:alee1:be";
        let parser = &mut Parser::new();
        let node = parser.parse(s).unwrap();
        let list = node.as_list().unwrap();
        let n = list.get(1).unwrap();
        assert_eq!(b"b", n.as_raw_bytes());
    }

    #[test]
    fn list_get_nested() {
        let s = b"l1:ad1:al1:aee1:be";
        let parser = &mut Parser::new();
        let node = parser.parse(s).unwrap();
        let node = node.as_list().unwrap();
        assert_eq!(b"a", node.get(0).unwrap().as_raw_bytes());
        assert_eq!(b"d1:al1:aee", node.get(1).unwrap().as_raw_bytes());
        assert_eq!(b"b", node.get(2).unwrap().as_raw_bytes());
        assert_eq!(None, node.get(3));
    }

    #[test]
    fn list_get_overflow() {
        let s = b"l1:al1:ad1:al1:aee1:be1:be";
        let parser = &mut Parser::new();
        let node = parser.parse(s).unwrap();
        let node = node.as_list().unwrap();
        let node = node.get_list(1).unwrap();
        assert_eq!(b"a", node.get(0).unwrap().as_raw_bytes());
        assert_eq!(b"d1:al1:aee", node.get(1).unwrap().as_raw_bytes());
        assert_eq!(b"b", node.get(2).unwrap().as_raw_bytes());
        assert_eq!(None, node.get(3));
    }

    #[test]
    fn list_iter() {
        let s = b"l1:ad1:al1:aee1:be";
        let parser = &mut Parser::new();
        let node = parser.parse(s).unwrap();
        let mut iter = node.as_list().unwrap().iter();
        assert_eq!(b"a", iter.next().unwrap().as_raw_bytes());
        assert_eq!(b"d1:al1:aee", iter.next().unwrap().as_raw_bytes());
        assert_eq!(b"b", iter.next().unwrap().as_raw_bytes());
        assert_eq!(None, iter.next());
    }

    #[test]
    fn list_iter_not_a_list() {
        let s = b"de";
        let parser = &mut Parser::new();
        let node = parser.parse(s).unwrap();
        let node = node.as_list();
        assert!(node.is_none());
    }

    #[test]
    fn dict_iter() {
        let s = b"d1:a2:bc3:def4:ghije";
        let parser = &mut Parser::new();
        let node = parser.parse(s).unwrap();
        let mut iter = node.as_dict().unwrap().iter();

        let (k, v) = iter.next().unwrap();
        assert_eq!(b"a", k.as_raw_bytes());
        assert_eq!(b"bc", v.as_raw_bytes());

        let (k, v) = iter.next().unwrap();
        assert_eq!(b"def", k.as_raw_bytes());
        assert_eq!(b"ghij", v.as_raw_bytes());

        assert_eq!(None, iter.next());
    }

    #[test]
    fn dict_iter_2() {
        let s = b"d1:alee";
        let parser = &mut Parser::new();
        let node = parser.parse(s).unwrap();
        let mut iter = node.as_dict().unwrap().iter();

        let (k, v) = iter.next().unwrap();
        assert_eq!(b"a", k.as_raw_bytes());
        assert_eq!(b"le", v.as_raw_bytes());

        assert_eq!(None, iter.next());
    }

    #[test]
    fn dict_iter_inside_list() {
        let s = b"ld1:alee1:a1:ae";
        let parser = &mut Parser::new();
        let node = parser.parse(s).unwrap();
        let mut list_iter = node.as_list().unwrap().iter();

        let dict = list_iter.next().unwrap();
        assert_eq!(b"a", list_iter.next().unwrap().as_raw_bytes());
        assert_eq!(b"a", list_iter.next().unwrap().as_raw_bytes());
        assert_eq!(None, list_iter.next());

        let mut iter = dict.as_dict().unwrap().iter();

        let (k, v) = iter.next().unwrap();
        assert_eq!(b"a", k.as_raw_bytes());
        assert_eq!(b"le", v.as_raw_bytes());

        assert_eq!(None, iter.next());
    }

    #[test]
    fn int_value() {
        let s = b"i12e";
        let parser = &mut Parser::new();
        let node = parser.parse(s).unwrap();
        assert_eq!(12, node.as_int().unwrap());
    }

    #[test]
    fn int_value_negative() {
        let s = b"i-12e";
        let parser = &mut Parser::new();
        let node = parser.parse(s).unwrap();
        assert_eq!(-12, node.as_int().unwrap());
    }

    #[test]
    fn int_value_invalid() {
        let s = b"ixyze";
        let err = Parser::new().parse(s).unwrap_err();
        assert_eq!(Error::Unexpected { pos: 1 }, err);
    }

    #[test]
    fn str_value() {
        let s = b"5:abcde";
        let parser = &mut Parser::new();
        let node = parser.parse(s).unwrap();
        assert_eq!(b"abcde", node.as_bytes().unwrap());
    }

    #[test]
    fn dict_get() {
        let s = b"d1:ai1e1:bi2ee";
        let parser = &mut Parser::new();
        let node = parser.parse(s).unwrap();
        let dict = node.as_dict().unwrap();
        let b = dict.get(b"b").unwrap();
        assert_eq!(2, b.as_int().unwrap());
    }

    #[test]
    fn dict_get_invalid() {
        let s = b"d1:ai1e1:bi2ee";
        let parser = &mut Parser::new();
        let node = parser.parse(s).unwrap();
        let dict = node.as_dict().unwrap();
        assert!(dict.get_dict(b"b").is_none());
        assert!(dict.get_list(b"b").is_none());
    }

    #[test]
    fn list_get_invalid() {
        let s = b"l1:a1:be";
        let parser = &mut Parser::new();
        let node = parser.parse(s).unwrap();
        let dict = node.as_list().unwrap();
        assert!(dict.get_dict(0).is_none());
        assert!(dict.get_list(1).is_none());
    }

    #[test]
    fn decode_empty() {
        let parser = &mut Parser::new();
        let err = parser.parse(&[]).unwrap_err();
        assert_eq!(err, Error::Eof);
    }

    #[test]
    fn decode_debug_bytes() {
        let s = "3:\x01\x01\x01".as_bytes();
        let parser = &mut Parser::new();
        let n = parser.parse(s).unwrap();
        assert!(n.as_bytes().is_some());
        assert!(n.as_ascii_str().is_none());
        assert_eq!("`Bytes:[1, 1, 1]`", format!("{:?}", n));
    }

    #[test]
    fn decode_debug_str() {
        let s = "3:abc".as_bytes();
        let parser = &mut Parser::new();
        let n = parser.parse(s).unwrap();
        assert!(n.as_bytes().is_some());
        assert!(n.as_ascii_str().is_some());
        assert_eq!("\"abc\"", format!("{:?}", n));
    }

    #[test]
    fn empty_dict_len() {
        let s = b"de";
        let parser = &mut Parser::new();
        let node = parser.parse(s).unwrap();
        assert!(node.as_dict().unwrap().is_empty());
    }

    #[test]
    fn non_empty_dict_len() {
        let s = b"d1:a1:be";
        let parser = &mut Parser::new();
        let node = parser.parse(s).unwrap();
        assert!(!node.as_dict().unwrap().is_empty());
        assert_eq!(node.as_dict().unwrap().len(), 1);
    }

    #[test]
    fn non_empty_dict_nested_len() {
        let s = b"d1:al1:ad1:al1:aee1:bee";
        let parser = &mut Parser::new();
        let node = parser.parse(s).unwrap();
        assert!(!node.as_dict().unwrap().is_empty());
        assert_eq!(node.as_dict().unwrap().len(), 1);
    }
    #[test]
    fn empty_list_len() {
        let s = b"le";
        let parser = &mut Parser::new();
        let node = parser.parse(s).unwrap();
        assert!(node.as_list().unwrap().is_empty());
    }

    #[test]
    fn non_empty_list_len() {
        let s = b"l1:a1:be";
        let parser = &mut Parser::new();
        let node = parser.parse(s).unwrap();
        assert!(!node.as_list().unwrap().is_empty());
        assert_eq!(node.as_list().unwrap().len(), 2);
    }

    #[test]
    fn non_empty_list_nested_len() {
        let s = b"l1:ad1:al1:aee1:be";
        let parser = &mut Parser::new();
        let node = parser.parse(s).unwrap();
        assert!(!node.as_list().unwrap().is_empty());
        assert_eq!(node.as_list().unwrap().len(), 3);
    }
}
