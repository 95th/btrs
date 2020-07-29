use crate::decode::{Decode, Decoder};
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
            token_limit: usize::max_value(),
            depth_limit: usize::max_value(),
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
    pub fn parse<'a, 'p, T>(&'p mut self, buf: &'a [u8]) -> Result<T>
    where
        T: Decode<'a, 'p>,
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
    pub fn parse_prefix<'a, 'p, T>(&'p mut self, buf: &'a [u8]) -> Result<(T, usize)>
    where
        T: Decode<'a, 'p>,
    {
        let (dec, pos) = self.parse_prefix_impl(buf)?;
        let t = T::decode(dec)?;
        Ok((t, pos))
    }

    fn parse_prefix_impl<'a, 'p>(&'p mut self, buf: &'a [u8]) -> Result<(Decoder<'a, 'p>, usize)> {
        if buf.is_empty() {
            return Err(Error::Eof);
        }

        self.tokens.clear();
        let mut state = State {
            buf,
            pos: 0,
            tokens: &mut self.tokens,
            token_limit: self.token_limit,
            depth_limit: self.depth_limit,
            current_depth: 0,
        };

        state.parse_object()?;
        let pos = state.pos;
        let d = Decoder::new(buf, &self.tokens).ok_or_else(|| Error::Eof)?;
        Ok((d, pos))
    }
}

struct State<'a, 't> {
    buf: &'a [u8],
    pos: usize,
    tokens: &'t mut Vec<Token>,
    token_limit: usize,
    depth_limit: usize,
    current_depth: usize,
}

impl<'a, 't> State<'a, 't> {
    fn peek_char(&self) -> Result<u8> {
        self.buf.get(self.pos).copied().ok_or_else(|| Error::Eof)
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
            b'0'..=b'9' => self.parse_string(),
            _ => Err(Error::Unexpected { pos: self.pos }),
        };

        self.current_depth -= 1;
        result
    }

    fn parse_dict(&mut self) -> Result<()> {
        let token_pos = self.create_token(TokenKind::Dict)?;

        // Consume the opening 'd'
        self.next_char()?;

        let mut children = 0;
        while self.peek_char()? != b'e' {
            self.parse_string()?;
            self.parse_object()?;
            children += 1;
        }

        // Consume the closing 'e'
        self.next_char()?;

        let next = self.tokens.len() - token_pos;
        let token = &mut self.tokens[token_pos];
        token.end = self.pos as u32;
        token.children = children;
        token.next = next as u32;

        Ok(())
    }

    fn parse_list(&mut self) -> Result<()> {
        let token_pos = self.create_token(TokenKind::List)?;

        // Consume the opening 'l'
        self.next_char()?;

        let mut children = 0;
        while self.peek_char()? != b'e' {
            self.parse_object()?;
            children += 1;
        }

        // Consume the closing 'e'
        self.next_char()?;

        let next = self.tokens.len() - token_pos;
        let token = &mut self.tokens[token_pos];
        token.end = self.pos as u32;
        token.children = children;
        token.next = next as u32;

        Ok(())
    }

    fn parse_int(&mut self) -> Result<()> {
        // Consume the opening 'i'
        self.next_char()?;

        let token_pos = self.create_token(TokenKind::Int)?;

        // Can be negative
        if self.peek_char()? == b'-' {
            self.pos += 1;
        }

        if self.peek_char()? == b'e' {
            return Err(Error::Unexpected { pos: self.pos });
        }

        let mut val: i64 = 0;

        loop {
            match self.next_char()? {
                c @ b'0'..=b'9' => {
                    let digit = i64::from(c - b'0');
                    match val.checked_mul(10).and_then(|n| n.checked_add(digit)) {
                        Some(n) => val = n,
                        None => return Err(Error::Overflow { pos: self.pos }),
                    }
                }
                b'e' => {
                    self.tokens[token_pos].end = (self.pos - 1) as u32;
                    return Ok(());
                }
                _ => return Err(Error::Unexpected { pos: self.pos - 1 }),
            }
        }
    }

    fn parse_string(&mut self) -> Result<()> {
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

        if self.pos + len <= self.buf.len() {
            let token_pos = self.create_token(TokenKind::ByteStr)?;
            self.pos += len;
            self.tokens[token_pos].end = self.pos as u32;
            Ok(())
        } else {
            Err(Error::Eof)
        }
    }

    fn create_token(&mut self, kind: TokenKind) -> Result<usize> {
        if self.tokens.len() == self.token_limit {
            return Err(Error::TokenLimit {
                limit: self.token_limit,
            });
        }
        let token = Token::new(kind, self.pos as u32, self.pos as u32);
        self.tokens.push(token);
        Ok(self.tokens.len() - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_int() {
        let s = b"i12e";
        let mut parser = Parser::new();
        parser.parse::<Decoder>(s).unwrap();
        assert_eq!(&[Token::new(TokenKind::Int, 1, 3)], &parser.tokens[..]);
    }

    #[test]
    fn parse_string() {
        let s = b"3:abc";
        let mut parser = Parser::new();
        parser.parse::<Decoder>(s).unwrap();
        assert_eq!(&[Token::new(TokenKind::ByteStr, 2, 5)], &parser.tokens[..]);
    }

    #[test]
    fn parse_string_too_long() {
        let s = b"3:abcd";
        let mut parser = Parser::new();
        let err = parser.parse::<Decoder>(s).unwrap_err();
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
        let err = parser.parse::<Decoder>(s).unwrap_err();
        assert_eq!(Error::Eof, err);
    }

    #[test]
    fn empty_dict() {
        let s = b"de";
        let mut parser = Parser::new();
        parser.parse::<Decoder>(s).unwrap();
        assert_eq!(&[Token::new(TokenKind::Dict, 0, 2)], &parser.tokens[..]);
    }

    #[test]
    fn unclosed_dict() {
        let s = b"d";
        let err = Parser::new().parse::<Decoder>(s).unwrap_err();
        assert_eq!(Error::Eof, err);
    }

    #[test]
    fn key_only_dict() {
        let s = b"d1:ae";
        let err = Parser::new().parse::<Decoder>(s).unwrap_err();
        assert_eq!(Error::Unexpected { pos: 4 }, err);
    }

    #[test]
    fn key_only_dict_2() {
        let s = b"d1:a1:a1:ae";
        let err = Parser::new().parse::<Decoder>(s).unwrap_err();
        assert_eq!(Error::Unexpected { pos: 10 }, err);
    }

    #[test]
    fn dict_string_values() {
        let s = b"d1:a2:ab3:abc4:abcde";
        let mut parser = Parser::new();
        parser.parse::<Decoder>(s).unwrap();
        assert_eq!(
            &[
                Token::with_size(TokenKind::Dict, 0, 20, 2, 5),
                Token::with_size(TokenKind::ByteStr, 3, 4, 0, 1),
                Token::with_size(TokenKind::ByteStr, 6, 8, 0, 1),
                Token::with_size(TokenKind::ByteStr, 10, 13, 0, 1),
                Token::with_size(TokenKind::ByteStr, 15, 19, 0, 1)
            ],
            &parser.tokens[..]
        );
    }

    #[test]
    fn dict_mixed_values() {
        let s = b"d1:a1:b1:ci1e1:x1:y1:dde1:fle1:g1:he";
        let mut parser = Parser::new();
        parser.parse::<Decoder>(s).unwrap();
        assert_eq!(
            &[
                Token::with_size(TokenKind::Dict, 0, 36, 6, 13),
                Token::with_size(TokenKind::ByteStr, 3, 4, 0, 1),
                Token::with_size(TokenKind::ByteStr, 6, 7, 0, 1),
                Token::with_size(TokenKind::ByteStr, 9, 10, 0, 1),
                Token::with_size(TokenKind::Int, 11, 12, 0, 1),
                Token::with_size(TokenKind::ByteStr, 15, 16, 0, 1),
                Token::with_size(TokenKind::ByteStr, 18, 19, 0, 1),
                Token::with_size(TokenKind::ByteStr, 21, 22, 0, 1),
                Token::with_size(TokenKind::Dict, 22, 24, 0, 1),
                Token::with_size(TokenKind::ByteStr, 26, 27, 0, 1),
                Token::with_size(TokenKind::List, 27, 29, 0, 1),
                Token::with_size(TokenKind::ByteStr, 31, 32, 0, 1),
                Token::with_size(TokenKind::ByteStr, 34, 35, 0, 1)
            ],
            &parser.tokens[..]
        );
    }

    #[test]
    fn empty_list() {
        let s = b"le";
        let mut parser = Parser::new();
        parser.parse::<Decoder>(s).unwrap();
        assert_eq!(&[Token::new(TokenKind::List, 0, 2)], &parser.tokens[..]);
    }

    #[test]
    fn unclosed_list() {
        let s = b"l";
        let err = Parser::new().parse::<Decoder>(s).unwrap_err();
        assert_eq!(Error::Eof, err);
    }

    #[test]
    fn list_string_values() {
        let s = b"l1:a2:ab3:abc4:abcde";
        let mut parser = Parser::new();
        parser.parse::<Decoder>(s).unwrap();
        assert_eq!(
            &[
                Token::with_size(TokenKind::List, 0, 20, 4, 5),
                Token::new(TokenKind::ByteStr, 3, 4),
                Token::new(TokenKind::ByteStr, 6, 8,),
                Token::new(TokenKind::ByteStr, 10, 13,),
                Token::new(TokenKind::ByteStr, 15, 19,)
            ],
            &parser.tokens[..]
        );
    }

    #[test]
    fn list_nested() {
        let s = b"lllleeee";
        let mut parser = Parser::new();
        parser.parse::<Decoder>(s).unwrap();
        assert_eq!(
            &[
                Token::with_size(TokenKind::List, 0, 8, 1, 4),
                Token::with_size(TokenKind::List, 1, 7, 1, 3),
                Token::with_size(TokenKind::List, 2, 6, 1, 2),
                Token::with_size(TokenKind::List, 3, 5, 0, 1),
            ],
            &parser.tokens[..]
        );
    }

    #[test]
    fn list_nested_complex() {
        let s = b"ld1:ald2:ablleeeeee";
        let mut parser = Parser::new();
        parser.parse::<Decoder>(s).unwrap();
        assert_eq!(
            &[
                Token::with_size(TokenKind::List, 0, 19, 1, 8),
                Token::with_size(TokenKind::Dict, 1, 18, 1, 7),
                Token::with_size(TokenKind::ByteStr, 4, 5, 0, 1),
                Token::with_size(TokenKind::List, 5, 17, 1, 5),
                Token::with_size(TokenKind::Dict, 6, 16, 1, 4),
                Token::with_size(TokenKind::ByteStr, 9, 11, 0, 1),
                Token::with_size(TokenKind::List, 11, 15, 1, 2),
                Token::with_size(TokenKind::List, 12, 14, 0, 1),
            ],
            &parser.tokens[..]
        );
    }

    #[test]
    fn token_limit() {
        let mut parser = Parser::new();
        parser.token_limit(3);

        let s = b"l1:a2:ab3:abc4:abcde";
        let err = parser.parse::<Decoder>(s).unwrap_err();
        assert_eq!(Error::TokenLimit { limit: 3 }, err);

        let decoder = parser.parse::<Decoder>(b"le").unwrap();
        assert_eq!(b"le", decoder.as_raw_bytes());
    }

    #[test]
    fn depth_limit() {
        let mut parser = Parser::new();
        parser.depth_limit(3);

        let err = parser.parse::<Decoder>(b"lllleeee").unwrap_err();
        assert_eq!(Error::DepthLimit { limit: 3 }, err);

        let decoder = parser.parse::<Decoder>(b"llleee").unwrap();
        assert_eq!(b"llleee", decoder.as_raw_bytes());

        let decoder = parser.parse::<Decoder>(b"ld1:aleee").unwrap();
        assert_eq!(b"ld1:aleee", decoder.as_raw_bytes());
    }

    #[test]
    fn multiple_root_tokens() {
        let mut parser = Parser::new();
        assert_eq!(
            Error::Invalid {
                reason: "Extra bytes at the end",
                pos: 3,
            },
            parser.parse::<Decoder>(b"1:a1:b").unwrap_err()
        );
        assert_eq!(
            Error::Invalid {
                reason: "Extra bytes at the end",
                pos: 3,
            },
            parser.parse::<Decoder>(b"i1e1:b").unwrap_err()
        );
        assert_eq!(
            Error::Invalid {
                reason: "Extra bytes at the end",
                pos: 5,
            },
            parser.parse::<Decoder>(b"l1:aede").unwrap_err()
        );
        assert_eq!(
            Error::Invalid {
                reason: "Extra bytes at the end",
                pos: 2,
            },
            parser.parse::<Decoder>(b"lel1:ae").unwrap_err()
        );
    }

    #[test]
    fn parse_prefix() {
        let s = b"lede";
        let mut parser = Parser::new();
        let (_, len) = parser.parse_prefix::<Decoder>(s).unwrap();
        assert_eq!(
            &[Token::with_size(TokenKind::List, 0, 2, 0, 1)],
            &parser.tokens[..]
        );
        assert_eq!(2, len);
    }

    #[test]
    fn parse_prefix_in() {
        let s = b"lede";
        let mut parser = Parser::new();
        let (_, len) = parser.parse_prefix::<Decoder>(s).unwrap();
        assert_eq!(
            &[Token::with_size(TokenKind::List, 0, 2, 0, 1)],
            &parser.tokens[..]
        );
        assert_eq!(2, len);
    }

    #[test]
    fn parse_empty_string() {
        let s = b"0:";
        let mut parser = Parser::new();
        parser.parse::<Decoder>(s).unwrap();
        assert_eq!(
            &[Token::with_size(TokenKind::ByteStr, 2, 2, 0, 1)],
            &parser.tokens[..]
        );
    }
}
