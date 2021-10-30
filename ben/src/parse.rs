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
            token_limit: self.token_limit,
            depth_limit: self.depth_limit,
            current_depth: 0,
        };

        state.parse_object()?;
        let pos = state.pos;
        let entry = Entry::new(buf, &self.tokens);
        Ok((entry, pos))
    }
}

struct ParserState<'a> {
    buf: &'a [u8],
    pos: usize,
    tokens: &'a mut Vec<Token>,
    token_limit: usize,
    depth_limit: usize,
    current_depth: usize,
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

    fn parse_object(&mut self) -> Result<()> {
        self.current_depth += 1;

        if self.current_depth > self.depth_limit {
            return Err(Error::DepthLimit {
                limit: self.depth_limit,
            });
        }

        let result = match self.peek_char()? {
            b'd' => self.parse_dict(),
            b'l' => self.parse_list(),
            b'i' => self.parse_int(),
            b'0'..=b'9' => self.parse_string(false),
            _ => Err(Error::Unexpected { pos: self.pos }),
        };

        self.current_depth -= 1;
        result
    }

    fn parse_dict(&mut self) -> Result<()> {
        let t = self.create_token(TokenKind::Dict)?;

        // Consume the opening 'd'
        self.next_char()?;

        while self.peek_char()? != b'e' {
            self.parse_string(true)?;
            self.parse_object()?;
        }

        // Consume the closing 'e'
        self.next_char()?;

        self.finish_token(t);

        Ok(())
    }

    fn parse_list(&mut self) -> Result<()> {
        let t = self.create_token(TokenKind::List)?;

        // Consume the opening 'l'
        self.next_char()?;

        while self.peek_char()? != b'e' {
            self.parse_object()?;
        }

        // Consume the closing 'e'
        self.next_char()?;

        self.finish_token(t);

        Ok(())
    }

    fn parse_int(&mut self) -> Result<()> {
        // Consume the opening 'i'
        self.next_char()?;

        let t = self.create_token(TokenKind::Int)?;

        // Can be negative
        if self.peek_char()? == b'-' {
            self.pos += 1;
        }

        if self.peek_char()? == b'e' {
            return Err(Error::Unexpected { pos: self.pos });
        }

        let mut val: i64 = 0;

        loop {
            match self.peek_char()? {
                c @ b'0'..=b'9' => {
                    let digit = i64::from(c - b'0');
                    match val.checked_mul(10).and_then(|n| n.checked_add(digit)) {
                        Some(n) => val = n,
                        None => return Err(Error::Overflow { pos: self.pos }),
                    }
                    self.pos += 1;
                }
                b'e' => {
                    self.finish_token(t);
                    self.pos += 1;
                    return Ok(());
                }
                _ => return Err(Error::Unexpected { pos: self.pos }),
            }
        }
    }

    fn parse_string(&mut self, validate_utf8: bool) -> Result<()> {
        let mut len: usize = 0;

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

        if self.pos + len > self.buf.len() {
            return Err(Error::Eof);
        }

        let t = self.create_token(TokenKind::ByteStr)?;
        let start = self.pos;
        self.pos += len;

        self.finish_token(t);

        if validate_utf8 {
            // Safety: We just went through this data.
            let value = unsafe { self.buf.get_unchecked(start..self.pos) };

            std::str::from_utf8(value).map_err(|_| Error::Invalid {
                pos: start,
                reason: "Dict key must be a valid UTF-8 string",
            })?;
        }

        Ok(())
    }

    fn create_token(&mut self, kind: TokenKind) -> Result<usize> {
        if self.tokens.len() == self.token_limit {
            return Err(Error::TokenLimit {
                limit: self.token_limit,
            });
        }

        self.tokens.push(Token::new(kind, self.pos as u32, 0, 1));
        Ok(self.tokens.len() - 1)
    }

    fn finish_token(&mut self, idx: usize) {
        let next = self.tokens.len() - idx;
        // Safety: Index is obtained by pushing a token. We never pop. It's always valid.
        let token = unsafe { self.tokens.get_unchecked_mut(idx) };
        token.finish(self.pos);
        token.next = next as u32;
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
