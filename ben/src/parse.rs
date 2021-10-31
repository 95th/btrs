use crate::decode::{Decode, Entry};
use crate::error::{Error, Result};
use crate::token::{Token, TokenKind};

/// Bencode Parser
pub struct Parser {
    tokens: Vec<Token>,
    token_limit: usize,
    depth_limit: usize,
}

impl Default for Parser {
    fn default() -> Self {
        Self {
            tokens: vec![],
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
    pub fn parse<'a, T>(&'a mut self, buf: &'a [u8]) -> Result<T>
    where
        T: Decode<'a>,
    {
        let (dec, len) = self.parse_prefix_impl(buf)?;
        if len == buf.len() {
            T::decode(dec)
        } else {
            Err(Error::Invalid {
                reason: "Extra bytes at the end",
                pos: len,
            })
        }
    }

    /// Parse one object from the beginning of given slice and return the parsed object and
    /// number of bytes processed.
    ///
    /// It's useful when there is trailing data after the bencoded bytes.
    pub fn parse_prefix<'a, T>(&'a mut self, buf: &'a [u8]) -> Result<(T, usize)>
    where
        T: Decode<'a>,
    {
        let (dec, pos) = self.parse_prefix_impl(buf)?;
        let t = T::decode(dec)?;
        Ok((t, pos))
    }

    fn parse_prefix_impl<'a>(&'a mut self, buf: &'a [u8]) -> Result<(Entry<'a>, usize)> {
        if buf.is_empty() {
            return Err(Error::Eof);
        }

        self.tokens.clear();
        let mut state = ParserState {
            buf,
            pos: 0,
            tokens: &mut self.tokens,
            scopes: vec![],
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

    /// Token is a dict
    is_dict: bool,

    /// We're expecting a key
    need_key: bool,
}

impl Scope {
    fn new(index: usize, dict: bool) -> Self {
        Self {
            index: index as u32,
            is_dict: dict,
            need_key: true,
        }
    }
}

struct ParserState<'a> {
    buf: &'a [u8],
    pos: usize,
    tokens: &'a mut Vec<Token>,
    scopes: Vec<Scope>,
    token_limit: usize,
    depth_limit: usize,
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
            let c = self.peek_char()?;

            if let Some(s) = self.scopes.last() {
                if s.is_dict && s.need_key && c != b'e' {
                    self.parse_string(true)?;
                    self.scopes.last_mut().unwrap().need_key = false;
                    continue;
                }
            }

            let cur_scope = self.scopes.len();

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
                b'e' => {
                    let s = self
                        .scopes
                        .pop()
                        .ok_or_else(|| Error::Unexpected { pos: self.pos })?;

                    if !s.need_key {
                        return Err(Error::Unexpected { pos: self.pos });
                    }

                    self.pos += 1;

                    let next = self.tokens.len() - s.index as usize;
                    let t = &mut self.tokens[s.index as usize];
                    t.finish(self.pos);
                    t.next = next as u32;
                }
                _ => return Err(Error::Unexpected { pos: self.pos }),
            }

            if cur_scope > 0 {
                if let Some(s) = self.scopes.get_mut(cur_scope - 1) {
                    if s.is_dict {
                        s.need_key = true;
                    }
                }
            }

            if self.scopes.is_empty() {
                break;
            }
        }

        if !self.scopes.is_empty() {
            return Err(Error::Unexpected { pos: self.pos });
        }

        Ok(())
    }

    fn parse_int(&mut self) -> Result<()> {
        // Consume the opening 'i'
        self.next_char()?;

        let start = self.pos;
        let mut sign = 1;

        // Can be negative
        if self.peek_char()? == b'-' {
            sign = -1;
            self.pos += 1;
        }

        if self.peek_char()? == b'e' {
            return Err(Error::Unexpected { pos: self.pos });
        }

        let mut val = 0_i64;
        loop {
            match self.peek_char()? {
                c @ b'0'..=b'9' => {
                    let d = i64::from(c - b'0');
                    match val.checked_mul(10).and_then(|n| n.checked_add(d)) {
                        Some(n) => val = n,
                        None => return Err(Error::Overflow { pos: self.pos }),
                    }
                    self.pos += 1;
                }
                b'e' => break,
                _ => return Err(Error::Unexpected { pos: self.pos }),
            }
        }

        if val.checked_mul(sign).is_none() {
            return Err(Error::Overflow { pos: self.pos });
        }

        // Consume the closing 'e'
        self.next_char()?;

        let len = self.pos - start - 1;
        let t = Token::new(TokenKind::Int, start as u32, len as u32, 1);
        self.create_token(t)
    }

    fn parse_string(&mut self, validate_utf8: bool) -> Result<()> {
        let mut len: usize = 0;

        if !self.peek_char()?.is_ascii_digit() {
            return Err(Error::Unexpected { pos: self.pos });
        }

        loop {
            match self.next_char()? {
                c @ b'0'..=b'9' => {
                    let digit = (c - b'0') as usize;
                    match len.checked_mul(10).and_then(|n| n.checked_add(digit)) {
                        Some(n) => len = n,
                        None => return Err(Error::Overflow { pos: self.pos }),
                    }
                }
                b':' => break,
                _ => return Err(Error::Unexpected { pos: self.pos }),
            }
        }

        if len > self.buf.len() - self.pos {
            return Err(Error::Eof);
        }

        let t = Token::new(TokenKind::ByteStr, self.pos as u32, len as u32, 1);
        self.create_token(t)?;

        let start = self.pos;
        self.pos += len;

        if validate_utf8 {
            let value = &self.buf[start..self.pos];

            std::str::from_utf8(value).map_err(|_| Error::Invalid {
                pos: start,
                reason: "Dict key must be a valid UTF-8 string",
            })?;
        }

        Ok(())
    }

    fn create_token(&mut self, token: Token) -> Result<()> {
        if self.tokens.len() >= self.token_limit {
            return Err(Error::TokenLimit {
                limit: self.token_limit,
            });
        }

        if let TokenKind::Dict | TokenKind::List = token.kind {
            if self.scopes.len() >= self.depth_limit {
                return Err(Error::DepthLimit {
                    limit: self.depth_limit,
                });
            }

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
    fn parse_int() {
        let s = b"i12e";
        let mut parser = Parser::new();
        parser.parse::<Entry>(s).unwrap();
        assert_eq!(&[Token::new(TokenKind::Int, 1, 2, 1)], &parser.tokens[..]);
    }

    #[test]
    fn parse_int_overflow() {
        let s = b"i9223372036854775808e";
        let mut parser = Parser::new();
        let err = parser.parse::<Entry>(s).unwrap_err();
        assert_eq!(err, Error::Overflow { pos: 19 });
    }

    #[test]
    fn parse_int_overflow_negative() {
        let s = b"i-9223372036854775809e";
        let mut parser = Parser::new();
        let err = parser.parse::<Entry>(s).unwrap_err();
        assert_eq!(err, Error::Overflow { pos: 20 });
    }

    #[test]
    fn parse_int_empty_token() {
        let s = b"ie";
        let mut parser = Parser::new();
        let err = parser.parse::<Entry>(s).unwrap_err();
        assert_eq!(err, Error::Unexpected { pos: 1 });
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
        assert_eq!(
            Error::Invalid {
                reason: "Extra bytes at the end",
                pos: 5,
            },
            err
        );
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
        assert_eq!(Error::Overflow { pos: s.len() - 1 }, err);
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
        assert_eq!(Error::Unexpected { pos: 4 }, err);
    }

    #[test]
    fn key_only_dict_2() {
        let s = b"d1:a1:a1:ae";
        let err = Parser::new().parse::<Entry>(s).unwrap_err();
        assert_eq!(Error::Unexpected { pos: 10 }, err);
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
        assert_eq!(
            err,
            Error::Invalid {
                pos: 3,
                reason: "Dict key must be a valid UTF-8 string"
            }
        );
    }

    #[test]
    fn dict_mixed_values() {
        let s = b"d1:a1:b1:ci1e1:x1:y1:dde1:fle1:g1:he";
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
        assert_eq!(Error::TokenLimit { limit: 3 }, err);

        let entry = parser.parse::<Entry>(b"le").unwrap();
        assert_eq!(b"le", entry.as_raw_bytes());
    }

    #[test]
    fn depth_limit() {
        let mut parser = Parser::new();
        parser.depth_limit(3);

        let err = parser.parse::<Entry>(b"lllleeee").unwrap_err();
        assert_eq!(Error::DepthLimit { limit: 3 }, err);

        let entry = parser.parse::<Entry>(b"llleee").unwrap();
        assert_eq!(b"llleee", entry.as_raw_bytes());

        let entry = parser.parse::<Entry>(b"ld1:aleee").unwrap();
        assert_eq!(b"ld1:aleee", entry.as_raw_bytes());
    }

    #[test]
    fn multiple_root_tokens() {
        let mut parser = Parser::new();
        assert_eq!(
            Error::Invalid {
                reason: "Extra bytes at the end",
                pos: 3,
            },
            parser.parse::<Entry>(b"1:a1:b").unwrap_err()
        );
        assert_eq!(
            Error::Invalid {
                reason: "Extra bytes at the end",
                pos: 3,
            },
            parser.parse::<Entry>(b"i1e1:b").unwrap_err()
        );
        assert_eq!(
            Error::Invalid {
                reason: "Extra bytes at the end",
                pos: 5,
            },
            parser.parse::<Entry>(b"l1:aede").unwrap_err()
        );
        assert_eq!(
            Error::Invalid {
                reason: "Extra bytes at the end",
                pos: 2,
            },
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
}
