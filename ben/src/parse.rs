use crate::decode::{Decode, Entry};
use crate::error::{Error, Result};
use crate::token::{Token, TokenKind};

/// Bencode Parser
pub struct Parser {
    tokens: Vec<Token>,
    scopes: Vec<Scope>,
    token_limit: usize,
    depth_limit: usize,
}

impl Default for Parser {
    fn default() -> Self {
        Self {
            tokens: vec![],
            scopes: vec![],
            token_limit: usize::MAX,
            depth_limit: usize::MAX,
        }
    }
}

impl Parser {
    /// Create a new Bencode parser.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new Bencode parser with given initial token capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            tokens: Vec::with_capacity(capacity),
            ..Self::default()
        }
    }

    /// Set a limit on number of tokens that can be created during parsing.
    pub fn token_limit(&mut self, token_limit: usize) {
        self.token_limit = token_limit;
    }

    /// Set a limit on depth of object nesting that is allowed during parsing.
    pub fn depth_limit(&mut self, depth_limit: usize) {
        self.depth_limit = depth_limit
    }

    /// Parse a bencoded slice and returns the parsed object
    pub fn parse<'b, 'p, T>(&'p mut self, buf: &'b [u8]) -> Result<T>
    where
        T: Decode<'b, 'p>,
    {
        let (dec, len) = self.parse_prefix_impl(buf)?;
        if len == buf.len() {
            T::decode(dec).ok_or(Error::Decode)
        } else {
            Err(Error::TrailingData)
        }
    }

    /// Parse one object from the beginning of given slice and return the parsed object and
    /// number of bytes processed.
    ///
    /// It's useful when there is trailing data after the bencoded bytes.
    pub fn parse_prefix<'b, 'p, T>(&'p mut self, buf: &'b [u8]) -> Result<(T, usize)>
    where
        T: Decode<'b, 'p>,
    {
        let (dec, pos) = self.parse_prefix_impl(buf)?;
        let t = T::decode(dec).ok_or(Error::Decode)?;
        Ok((t, pos))
    }

    fn parse_prefix_impl<'b, 'p>(&'p mut self, buf: &'b [u8]) -> Result<(Entry<'b, 'p>, usize)> {
        self.tokens.clear();
        self.scopes.clear();

        let mut state = ParserState {
            buf,
            pos: 0,
            tokens: &mut self.tokens,
            scopes: &mut self.scopes,
            token_limit: self.token_limit,
            depth_limit: self.depth_limit,
        };

        state.parse()?;
        let pos = state.pos;
        let entry = Entry::new(buf, &self.tokens);
        Ok((entry, pos))
    }
}

struct Scope {
    /// Token index
    index: u32,

    /// Token is dictionary
    dict: bool,
}

impl Scope {
    fn new(index: usize, dict: bool) -> Self {
        Self {
            index: index as u32,
            dict,
        }
    }
}

struct ParserState<'a> {
    buf: &'a [u8],
    pos: usize,
    tokens: &'a mut Vec<Token>,
    scopes: &'a mut Vec<Scope>,
    token_limit: usize,
    depth_limit: usize,
}

macro_rules! ensure {
    ($cond:expr) => {
        ensure!($cond, Invalid);
    };
    ($cond:expr, $err:ident) => {
        if !$cond {
            return Err(Error::$err);
        }
    };
}

impl<'a> ParserState<'a> {
    fn peek_char(&self) -> Result<u8> {
        self.buf.get(self.pos).copied().ok_or(Error::Eof)
    }

    fn next_char(&mut self) -> Result<u8> {
        let c = self.peek_char()?;
        self.pos += 1;
        Ok(c)
    }

    fn parse(&mut self) -> Result<()> {
        loop {
            let mut c = self.peek_char()?;

            if let Some(s) = self.scopes.last() {
                if s.dict && c != b'e' {
                    // The key must be a string
                    ensure!(c.is_ascii_digit());

                    // Parse key as a valid UTF-8 string
                    self.parse_string(true)?;

                    c = self.peek_char()?;
                    ensure!(c != b'e');
                }
            }

            match c {
                b'd' => {
                    let t = Token::new(TokenKind::Dict, self.pos as u32, 2, 1);
                    self.create_token(t)?;
                    self.pos += 1;
                }
                b'l' => {
                    let t = Token::new(TokenKind::List, self.pos as u32, 2, 1);
                    self.create_token(t)?;
                    self.pos += 1;
                }
                b'i' => self.parse_int()?,
                b'0'..=b'9' => self.parse_string(false)?,
                b'e' => self.pop_scope()?,
                _ => return Err(Error::Invalid),
            }

            if self.scopes.is_empty() {
                break;
            }
        }

        ensure!(self.scopes.is_empty());
        Ok(())
    }

    fn pop_scope(&mut self) -> Result<()> {
        let s = self.scopes.pop().ok_or(Error::Invalid)?;

        self.pos += 1;

        let next = self.tokens.len() - s.index as usize;
        let t = &mut self.tokens[s.index as usize];
        t.finish(self.pos);
        t.next = next as u32;

        if s.dict {
            let dict = Entry::from_raw(self.buf.as_ptr(), t).as_dict().unwrap();
            let mut last_key = "";
            for (k, _) in dict {
                ensure!(last_key <= k);
                last_key = k;
            }
        }

        Ok(())
    }

    fn parse_int(&mut self) -> Result<()> {
        // Consume the opening 'i'
        self.next_char()?;

        let start = self.pos;
        let mut c = self.next_char()?;

        if c == b'-' {
            c = self.next_char()?;
            ensure!(c != b'0');
        }

        ensure!(c.is_ascii_digit());

        if c == b'0' {
            c = self.next_char()?;
        } else {
            while c.is_ascii_digit() {
                c = self.next_char()?;
            }
        }

        ensure!(c == b'e');

        let len = self.pos - start - 1;
        let t = Token::new(TokenKind::Int, start as u32, len as u32, 1);
        self.create_token(t)
    }

    fn parse_string(&mut self, validate_utf8: bool) -> Result<()> {
        let mut len: usize = 0;

        let mut c = self.next_char()?;
        if c == b'0' {
            c = self.next_char()?;
        } else {
            while c.is_ascii_digit() {
                let digit = (c - b'0') as usize;
                len = len
                    .checked_mul(10)
                    .and_then(|n| n.checked_add(digit))
                    .ok_or(Error::Overflow)?;

                c = self.next_char()?;
            }
        }

        ensure!(c == b':');
        ensure!(len <= self.buf.len() - self.pos, Eof);

        let t = Token::new(TokenKind::ByteStr, self.pos as u32, len as u32, 1);
        self.create_token(t)?;

        let start = self.pos;
        self.pos += len;

        if validate_utf8 {
            let value = &self.buf[start..self.pos];

            std::str::from_utf8(value).map_err(|_| Error::Invalid)?;
        }

        Ok(())
    }

    fn create_token(&mut self, token: Token) -> Result<()> {
        ensure!(self.tokens.len() < self.token_limit, TokenLimit);

        if let TokenKind::Dict | TokenKind::List = token.kind {
            ensure!(self.scopes.len() < self.depth_limit, DepthLimit);
            let s = Scope::new(self.tokens.len(), token.kind == TokenKind::Dict);
            self.scopes.push(s);
        }

        self.tokens.push(token);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_empty() {
        let s = b"";
        let mut parser = Parser::new();
        let err = parser.parse::<Entry>(s).unwrap_err();
        assert_eq!(err, Error::Eof);
    }

    #[test]
    fn parse_int() {
        let s = b"i12e";
        let mut parser = Parser::new();
        parser.parse::<Entry>(s).unwrap();
        assert_eq!(&[Token::new(TokenKind::Int, 1, 2, 1)], &parser.tokens[..]);
    }

    #[test]
    fn parse_int_huge_positive() {
        let s = b"i92233720368547758079223372036854775807922337203685477580792233720368547758079223372036854775807922337203685477580792233720368547758079223372036854775807922337203685477580792233720368547758079223372036854775807e";
        let mut parser = Parser::new();
        parser.parse::<Entry>(s).unwrap();
        assert_eq!(&[Token::new(TokenKind::Int, 1, 209, 1)], &parser.tokens[..]);
    }

    #[test]
    fn parse_int_huge_negative() {
        let s = b"i-92233720368547758079223372036854775807922337203685477580792233720368547758079223372036854775807922337203685477580792233720368547758079223372036854775807922337203685477580792233720368547758079223372036854775807e";
        let mut parser = Parser::new();
        parser.parse::<Entry>(s).unwrap();
        assert_eq!(&[Token::new(TokenKind::Int, 1, 210, 1)], &parser.tokens[..]);
    }

    #[test]
    fn parse_int_empty_token() {
        let s = b"ie";
        let mut parser = Parser::new();
        let err = parser.parse::<Entry>(s).unwrap_err();
        assert_eq!(err, Error::Invalid);
    }

    #[test]
    fn parse_string() {
        let s = b"3:abc";
        let mut parser = Parser::new();
        parser.parse::<Entry>(s).unwrap();
        assert_eq!(
            &[Token::new(TokenKind::ByteStr, 2, 3, 1)],
            &parser.tokens[..]
        );
    }

    #[test]
    fn parse_string_too_long() {
        let s = b"3:abcd";
        let mut parser = Parser::new();
        let err = parser.parse::<Entry>(s).unwrap_err();
        assert_eq!(Error::TrailingData, err);
    }

    #[test]
    fn parse_string_too_short() {
        let s = b"3:ab";
        let mut parser = Parser::new();
        let err = parser.parse::<Entry>(s).unwrap_err();
        assert_eq!(Error::Eof, err);
    }

    #[test]
    fn parse_string_length_overflow() {
        let s = format!("{}:", (usize::MAX as u128 + 1));
        let mut parser = Parser::new();
        let err = parser.parse::<Entry>(s.as_bytes()).unwrap_err();
        assert_eq!(Error::Overflow, err);
    }

    #[test]
    fn empty_dict() {
        let s = b"de";
        let mut parser = Parser::new();
        parser.parse::<Entry>(s).unwrap();
        assert_eq!(&[Token::new(TokenKind::Dict, 0, 2, 1)], &parser.tokens[..]);
    }

    #[test]
    fn unclosed_dict() {
        let s = b"d";
        let err = Parser::new().parse::<Entry>(s).unwrap_err();
        assert_eq!(Error::Eof, err);
    }

    #[test]
    fn key_only_dict() {
        let s = b"d1:ae";
        let err = Parser::new().parse::<Entry>(s).unwrap_err();
        assert_eq!(Error::Invalid, err);
    }

    #[test]
    fn key_only_dict_2() {
        let s = b"d1:a1:a1:ae";
        let err = Parser::new().parse::<Entry>(s).unwrap_err();
        assert_eq!(Error::Invalid, err);
    }

    #[test]
    fn dict_string_values() {
        let s = b"d1:a2:ab3:abc4:abcde";
        let mut parser = Parser::new();
        parser.parse::<Entry>(s).unwrap();
        assert_eq!(
            &[
                Token::new(TokenKind::Dict, 0, 20, 5),
                Token::new(TokenKind::ByteStr, 3, 1, 1),
                Token::new(TokenKind::ByteStr, 6, 2, 1),
                Token::new(TokenKind::ByteStr, 10, 3, 1),
                Token::new(TokenKind::ByteStr, 15, 4, 1)
            ],
            &parser.tokens[..]
        );
    }

    #[test]
    fn dict_non_utf8_key() {
        let s = &[b'd', b'1', b':', 0x80, b'2', b':', b'a', b'b', b'e'];
        let mut parser = Parser::new();
        let err = parser.parse::<Entry>(s).unwrap_err();
        assert_eq!(err, Error::Invalid);
    }

    #[test]
    fn dict_mixed_values() {
        let s = b"d1:a1:b1:ci1e1:d1:e1:fde1:gle1:g1:he";
        let mut parser = Parser::new();
        parser.parse::<Entry>(s).unwrap();
        assert_eq!(
            &[
                Token::new(TokenKind::Dict, 0, 36, 13),
                Token::new(TokenKind::ByteStr, 3, 1, 1),
                Token::new(TokenKind::ByteStr, 6, 1, 1),
                Token::new(TokenKind::ByteStr, 9, 1, 1),
                Token::new(TokenKind::Int, 11, 1, 1),
                Token::new(TokenKind::ByteStr, 15, 1, 1),
                Token::new(TokenKind::ByteStr, 18, 1, 1),
                Token::new(TokenKind::ByteStr, 21, 1, 1),
                Token::new(TokenKind::Dict, 22, 2, 1),
                Token::new(TokenKind::ByteStr, 26, 1, 1),
                Token::new(TokenKind::List, 27, 2, 1),
                Token::new(TokenKind::ByteStr, 31, 1, 1),
                Token::new(TokenKind::ByteStr, 34, 1, 1)
            ],
            &parser.tokens[..]
        );
    }

    #[test]
    fn empty_list() {
        let s = b"le";
        let mut parser = Parser::new();
        parser.parse::<Entry>(s).unwrap();
        assert_eq!(&[Token::new(TokenKind::List, 0, 2, 1)], &parser.tokens[..]);
    }

    #[test]
    fn unclosed_list() {
        let s = b"l";
        let err = Parser::new().parse::<Entry>(s).unwrap_err();
        assert_eq!(Error::Eof, err);
    }

    #[test]
    fn list_string_values() {
        let s = b"l1:a2:ab3:abc4:abcde";
        let mut parser = Parser::new();
        parser.parse::<Entry>(s).unwrap();
        assert_eq!(
            &[
                Token::new(TokenKind::List, 0, 20, 5),
                Token::new(TokenKind::ByteStr, 3, 1, 1),
                Token::new(TokenKind::ByteStr, 6, 2, 1),
                Token::new(TokenKind::ByteStr, 10, 3, 1),
                Token::new(TokenKind::ByteStr, 15, 4, 1)
            ],
            &parser.tokens[..]
        );
    }

    #[test]
    fn list_nested() {
        let s = b"lllleeee";
        let mut parser = Parser::new();
        parser.parse::<Entry>(s).unwrap();
        assert_eq!(
            &[
                Token::new(TokenKind::List, 0, 8, 4),
                Token::new(TokenKind::List, 1, 6, 3),
                Token::new(TokenKind::List, 2, 4, 2),
                Token::new(TokenKind::List, 3, 2, 1),
            ],
            &parser.tokens[..]
        );
    }

    #[test]
    fn list_nested_complex() {
        let s = b"ld1:ald2:ablleeeeee";
        let mut parser = Parser::new();
        parser.parse::<Entry>(s).unwrap();
        assert_eq!(
            &[
                Token::new(TokenKind::List, 0, 19, 8),
                Token::new(TokenKind::Dict, 1, 17, 7),
                Token::new(TokenKind::ByteStr, 4, 1, 1),
                Token::new(TokenKind::List, 5, 12, 5),
                Token::new(TokenKind::Dict, 6, 10, 4),
                Token::new(TokenKind::ByteStr, 9, 2, 1),
                Token::new(TokenKind::List, 11, 4, 2),
                Token::new(TokenKind::List, 12, 2, 1),
            ],
            &parser.tokens[..]
        );
    }

    #[test]
    fn token_limit() {
        let mut parser = Parser::new();
        parser.token_limit(3);

        let s = b"l1:a2:ab3:abc4:abcde";
        let err = parser.parse::<Entry>(s).unwrap_err();
        assert_eq!(Error::TokenLimit, err);

        let entry = parser.parse::<Entry>(b"le").unwrap();
        assert_eq!(b"le", entry.as_raw_bytes());
    }

    #[test]
    fn depth_limit() {
        let mut parser = Parser::new();
        parser.depth_limit(3);

        let err = parser.parse::<Entry>(b"lllleeee").unwrap_err();
        assert_eq!(Error::DepthLimit, err);

        let entry = parser.parse::<Entry>(b"llleee").unwrap();
        assert_eq!(b"llleee", entry.as_raw_bytes());

        let entry = parser.parse::<Entry>(b"ld1:aleee").unwrap();
        assert_eq!(b"ld1:aleee", entry.as_raw_bytes());
    }

    #[test]
    fn multiple_root_tokens() {
        let mut parser = Parser::new();
        assert_eq!(
            Error::TrailingData,
            parser.parse::<Entry>(b"1:a1:b").unwrap_err()
        );
        assert_eq!(
            Error::TrailingData,
            parser.parse::<Entry>(b"i1e1:b").unwrap_err()
        );
        assert_eq!(
            Error::TrailingData,
            parser.parse::<Entry>(b"l1:aede").unwrap_err()
        );
        assert_eq!(
            Error::TrailingData,
            parser.parse::<Entry>(b"lel1:ae").unwrap_err()
        );
    }

    #[test]
    fn parse_prefix() {
        let s = b"lede";
        let mut parser = Parser::new();
        let (_, len) = parser.parse_prefix::<Entry>(s).unwrap();
        assert_eq!(&[Token::new(TokenKind::List, 0, 2, 1)], &parser.tokens[..]);
        assert_eq!(2, len);
    }

    #[test]
    fn parse_prefix_in() {
        let s = b"lede";
        let mut parser = Parser::new();
        let (_, len) = parser.parse_prefix::<Entry>(s).unwrap();
        assert_eq!(&[Token::new(TokenKind::List, 0, 2, 1)], &parser.tokens[..]);
        assert_eq!(2, len);
    }

    #[test]
    fn parse_empty_string() {
        let s = b"0:";
        let mut parser = Parser::new();
        parser.parse::<Entry>(s).unwrap();
        assert_eq!(
            &[Token::new(TokenKind::ByteStr, 2, 0, 1)],
            &parser.tokens[..]
        );
    }

    #[test]
    fn reject_negative_zero() {
        let s = b"i-0e";
        let mut parser = Parser::new();
        let err = parser.parse::<Entry>(s).unwrap_err();
        assert_eq!(err, Error::Invalid);
    }

    #[test]
    fn reject_negative_twice() {
        let s = b"i--1e";
        let mut parser = Parser::new();
        let err = parser.parse::<Entry>(s).unwrap_err();
        assert_eq!(err, Error::Invalid);
    }

    #[test]
    fn reject_leading_zero_1() {
        let s = b"i000e";
        let mut parser = Parser::new();
        let err = parser.parse::<Entry>(s).unwrap_err();
        assert_eq!(err, Error::Invalid);
    }

    #[test]
    fn reject_leading_zero_2() {
        let s = b"i01e";
        let mut parser = Parser::new();
        let err = parser.parse::<Entry>(s).unwrap_err();
        assert_eq!(err, Error::Invalid);
    }

    #[test]
    fn reject_minus_without_digit() {
        let s = b"i-e";
        let mut parser = Parser::new();
        let err = parser.parse::<Entry>(s).unwrap_err();
        assert_eq!(err, Error::Invalid);
    }

    #[test]
    fn parse_zero_int() {
        let s = b"i0e";
        let mut parser = Parser::new();
        parser.parse::<Entry>(s).unwrap();
        assert_eq!(&[Token::new(TokenKind::Int, 1, 1, 1)], &parser.tokens[..]);
    }

    #[test]
    fn reject_string_leading_zeros() {
        let s = b"001:a";
        let mut parser = Parser::new();
        let err = parser.parse::<Entry>(s).unwrap_err();
        assert_eq!(err, Error::Invalid);
    }

    #[test]
    fn reject_string_no_length() {
        let s = b":";
        let mut parser = Parser::new();
        let err = parser.parse::<Entry>(s).unwrap_err();
        assert_eq!(err, Error::Invalid);
    }

    #[test]
    fn reject_dict_unsorted_keys() {
        let s = b"d1:b0:1:a0:e";
        let mut parser = Parser::new();
        let err = parser.parse::<Entry>(s).unwrap_err();
        assert_eq!(err, Error::Invalid);
    }

    #[test]
    fn reject_dict_unsorted_keys_nested_inside_dict() {
        let s = b"d1:ad1:b0:1:a0:ee";
        let mut parser = Parser::new();
        let err = parser.parse::<Entry>(s).unwrap_err();
        assert_eq!(err, Error::Invalid);
    }

    #[test]
    fn reject_dict_unsorted_keys_nested_inside_list() {
        let s = b"l1:ad1:b0:1:a0:ee";
        let mut parser = Parser::new();
        let err = parser.parse::<Entry>(s).unwrap_err();
        assert_eq!(err, Error::Invalid);
    }
}
