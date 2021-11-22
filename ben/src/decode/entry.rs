use std::{fmt, marker::PhantomData};

use crate::token::{Token, TokenKind};

use super::{Dict, Int, List};

#[derive(Clone, Copy, PartialEq)]
pub struct Entry<'b, 'p> {
    pub(crate) buf: *const u8,
    pub(crate) token: *const Token,
    _marker: PhantomData<(&'b (), &'p ())>,
}

impl fmt::Debug for Entry<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.token().kind {
            TokenKind::Int => write!(f, "{}", self.as_int::<i128>().unwrap()),
            TokenKind::ByteStr => match self.as_ascii_str() {
                Some(s) => write!(f, "\"{}\"", s),
                None => write!(f, "'{}'", data_encoding::BASE32.encode(self.as_raw_bytes())),
            },
            TokenKind::List => self.as_list().unwrap().fmt(f),
            TokenKind::Dict => self.as_dict().unwrap().fmt(f),
        }
    }
}

impl<'b, 'p> Entry<'b, 'p> {
    pub(crate) fn new(buf: &'b [u8], tokens: &'p [Token]) -> Self {
        Entry::from_raw(buf.as_ptr(), tokens.as_ptr())
    }

    pub(crate) fn from_raw(buf: *const u8, token: *const Token) -> Self {
        Entry {
            buf,
            token,
            _marker: PhantomData,
        }
    }

    pub(crate) fn token(&self) -> &Token {
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
    pub fn as_raw_bytes(&self) -> &'b [u8] {
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
    /// let list = entry.as_list().unwrap();
    /// assert_eq!(b"a", list.get_bytes(0).unwrap());
    /// assert_eq!(b"bc", list.get_bytes(1).unwrap());
    /// ```
    pub fn as_list(&self) -> Option<List<'b, 'p>> {
        if self.is_list() {
            Some(List { entry: *self })
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
    /// let dict = entry.as_dict().unwrap();
    /// assert_eq!(b"bc", dict.get_bytes("a").unwrap());
    /// ```
    pub fn as_dict(&self) -> Option<Dict<'b, 'p>> {
        if self.is_dict() {
            Some(Dict { entry: *self })
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
    pub fn as_int<I>(&self) -> Option<I>
    where
        I: Int,
    {
        if !self.is_int() {
            return None;
        }

        I::decode(self.as_raw_bytes())
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
    pub fn as_bytes(&self) -> Option<&'b [u8]> {
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
    pub fn as_str(&self) -> Option<&'b str> {
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
    /// let entry = parser.parse::<Entry>(b"1:\x80").unwrap();
    /// assert!(entry.as_ascii_str().is_none());
    /// ```
    pub fn as_ascii_str(&self) -> Option<&'b str> {
        let s = self.as_str()?;

        if s.is_ascii() {
            Some(s)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::*;
    use crate::Error;

    #[test]
    fn int_value() {
        let s = b"i12e";
        let p = &mut Parser::new();
        let n = p.parse::<i64>(s).unwrap();
        assert_eq!(12, n);
    }

    #[test]
    fn int_value_negative() {
        let s = b"i-12e";
        let p = &mut Parser::new();
        let n = p.parse::<i64>(s).unwrap();
        assert_eq!(-12, n);
    }

    #[test]
    fn int_value_negative_unexpected() {
        let s = b"i-12e";
        let p = &mut Parser::new();
        let n = p.parse::<u8>(s).unwrap_err();
        assert_eq!(n, Error::Decode);
    }

    #[test]
    fn int_value_overflow_positive() {
        let s = b"i256e";
        let p = &mut Parser::new();
        let n = p.parse::<u8>(s).unwrap_err();
        assert_eq!(n, Error::Decode);
    }

    #[test]
    fn int_value_overflow_negative() {
        let s = b"i128e";
        let p = &mut Parser::new();
        let n = p.parse::<i8>(s).unwrap_err();
        assert_eq!(n, Error::Decode);
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
        let p = &mut Parser::new();
        let bytes = p.parse::<&[u8]>(s).unwrap();
        assert_eq!(b"abcde", bytes);
    }

    #[test]
    fn decode_empty() {
        let p = &mut Parser::new();
        let err = p.parse::<Entry>(&[]).unwrap_err();
        assert_eq!(err, Error::Eof);
    }

    #[test]
    fn decode_debug_bytes() {
        let s = b"1:\x80";
        let p = &mut Parser::new();
        let n = p.parse::<Entry>(s).unwrap();
        assert!(n.as_bytes().is_some());
        assert!(n.as_ascii_str().is_none());
        assert_eq!(
            format!("'{}'", data_encoding::BASE32.encode(&[0x80])),
            format!("{:?}", n)
        );
    }

    #[test]
    fn decode_debug_str() {
        let s = b"3:abc";
        let p = &mut Parser::new();
        let n = p.parse::<Entry>(s).unwrap();
        assert!(n.as_bytes().is_some());
        assert!(n.as_ascii_str().is_some());
        assert_eq!("\"abc\"", format!("{:?}", n));
    }

    #[test]
    fn str_decode_lifetime() {
        let s = b"5:abcde";
        let val = {
            let p = &mut Parser::new();
            p.parse::<&str>(s).unwrap()
        };
        assert_eq!("abcde", val);
    }
}
