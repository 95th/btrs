use crate::decode::Node;
use crate::error::{Error, Result};
use crate::token::{Token, TokenKind};

/// Bencode Parser
pub struct Parser {
    pos: usize,
    tok_next: usize,
    tok_super: isize,
    token_limit: usize,
    tokens: Vec<Token>,
}

impl Default for Parser {
    fn default() -> Self {
        Self {
            pos: 0,
            tok_next: 0,
            tok_super: -1,
            token_limit: usize::max_value(),
            tokens: vec![],
        }
    }
}

impl Parser {
    /// Create a new Bencode parser
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new Bencode parser with limit on number of tokens that
    /// can be created during parsing.
    pub fn with_token_limit(token_limit: usize) -> Self {
        Self {
            token_limit,
            ..Self::default()
        }
    }

    /// Parse a bencoded slice and returns a `Node` object
    pub fn parse<'a>(&'a mut self, buf: &'a [u8]) -> Result<Node<'a>> {
        let (node, len) = self.parse_prefix(buf)?;
        if len == buf.len() {
            Ok(node)
        } else {
            Err(Error::Invalid {
                reason: "Extra bytes at the end",
                pos: len,
            })
        }
    }

    /// Parse one object from the beginning of given slice and return the parsed `Node` and
    /// number of bytes processed.
    ///
    /// It's useful when there is trailing data after the bencoded bytes.
    pub fn parse_prefix<'a>(&'a mut self, buf: &'a [u8]) -> Result<(Node<'a>, usize)> {
        if buf.is_empty() {
            return Err(Error::Eof);
        }

        self.clear();
        let mut depth = 0;
        while self.pos < buf.len() {
            let c = buf[self.pos];
            match c {
                b'i' => {
                    self.update_super(TokenKind::Int)?;
                    self.pos += 1;
                    let start = self.pos;
                    self.parse_int(buf, b'e')?;
                    let token = Token::new(TokenKind::Int, start as i32, self.pos as i32);
                    self.alloc_token(token)?;
                    self.pos += 1;
                }
                b'l' => {
                    depth += 1;
                    let token = Token::new(TokenKind::List, self.pos as i32, -1);
                    self.pos += 1;
                    self.alloc_token(token)?;
                    self.update_super(TokenKind::List)?;
                    self.tok_super = self.tok_next as isize - 1;
                }
                b'd' => {
                    depth += 1;
                    let token = Token::new(TokenKind::Dict, self.pos as i32, -1);
                    self.pos += 1;
                    self.alloc_token(token)?;
                    self.update_super(TokenKind::Dict)?;
                    self.tok_super = self.tok_next as isize - 1;
                }
                b'0'..=b'9' => {
                    self.parse_string(buf)?;
                    self.update_super(TokenKind::ByteStr)?;
                }
                b'e' => {
                    self.pos += 1;
                    depth -= 1;
                    let mut i = (self.tok_next - 1) as i32;
                    while i >= 0 {
                        let token = &mut self.tokens[i as usize];
                        if token.start >= 0 && token.end < 0 {
                            token.next = self.tok_next as u32 - i as u32;
                            self.tok_super = -1;
                            token.end = self.pos as i32;
                            break;
                        } else {
                            i -= 1
                        }
                    }

                    // Error if unclosed object
                    if i == -1 {
                        return Err(Error::Invalid {
                            reason: "Unclosed object",
                            pos: self.pos,
                        });
                    }

                    while i >= 0 {
                        let token = &self.tokens[i as usize];
                        if token.start >= 0 && token.end < 0 {
                            self.tok_super = i as isize;
                            break;
                        } else {
                            i -= 1
                        }
                    }
                }
                _ => {
                    // Unexpected char
                    return Err(Error::Unexpected { pos: self.pos });
                }
            }
            if depth == 0 {
                break;
            }
        }
        for i in (0..self.tok_next).rev() {
            let token = &self.tokens[i];

            // Unclosed object
            if token.start >= 0 && token.end < 0 {
                return Err(Error::Eof);
            }

            if let TokenKind::Dict = token.kind {
                if token.children % 2 != 0 {
                    return Err(Error::Eof);
                }
            }
        }
        let node = Node::new(buf, &self.tokens).ok_or_else(|| Error::Eof)?;
        Ok((node, self.pos))
    }

    fn clear(&mut self) {
        self.tokens.clear();
        self.pos = 0;
        self.tok_next = 0;
        self.tok_super = -1;
    }

    fn update_super(&mut self, curr_kind: TokenKind) -> Result<()> {
        if self.tok_super < 0 {
            return Ok(());
        }

        let parent = &mut self.tokens[self.tok_super as usize];
        parent.children += 1;
        if let TokenKind::Dict = parent.kind {
            if curr_kind != TokenKind::ByteStr && parent.children % 2 != 0 {
                return Err(Error::Invalid {
                    reason: "Dictionary key must be a string",
                    pos: self.pos,
                });
            }
        }
        Ok(())
    }

    /// Parse bencode int.
    fn parse_int(&mut self, buf: &[u8], stop_char: u8) -> Result<i64> {
        let mut negative = false;
        let mut pos = self.pos;

        if let Some(b'-') = buf.get(pos) {
            pos += 1;
            negative = true;
        }

        if pos >= buf.len() {
            return Err(Error::Eof);
        }

        let mut val = 0;
        while let Some(&c) = buf.get(pos) {
            if let b'0'..=b'9' = c {
                if val > i64::max_value() / 10 {
                    return Err(Error::Overflow { pos: self.pos });
                }
                val *= 10;
                let digit = (c - b'0') as i64;
                if val > i64::max_value() - digit {
                    return Err(Error::Overflow { pos: self.pos });
                }
                val += digit;
                pos += 1;
            } else if c == stop_char {
                break;
            } else {
                return Err(Error::Unexpected { pos });
            }
        }
        self.pos = pos;

        if negative {
            val *= -1;
        }

        Ok(val)
    }

    /// Fills next token with bencode string.
    fn parse_string(&mut self, buf: &[u8]) -> Result<()> {
        let len = self.parse_int(buf, b':')?;
        if len < 0 {
            return Err(Error::Invalid {
                reason: "String length must be positive",
                pos: self.pos,
            });
        }

        self.pos += 1; // Skip the ':'

        let len = len as usize;
        if self.pos + len > buf.len() {
            return Err(Error::Eof);
        }

        let token = Token::new(TokenKind::ByteStr, self.pos as i32, (self.pos + len) as i32);
        self.alloc_token(token)?;
        self.pos += len;
        Ok(())
    }

    /// Adds a new token.
    fn alloc_token(&mut self, token: Token) -> Result<()> {
        if self.tokens.len() >= self.token_limit {
            return Err(Error::NoMemory);
        }
        self.tokens.push(token);
        self.tok_next += 1;
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
        parser.parse(s).unwrap();
        assert_eq!(&[Token::new(TokenKind::Int, 1, 3)], &parser.tokens[..]);
    }

    #[test]
    fn parse_string() {
        let s = b"3:abc";
        let mut parser = Parser::new();
        parser.parse(s).unwrap();
        assert_eq!(&[Token::new(TokenKind::ByteStr, 2, 5)], &parser.tokens[..]);
    }

    #[test]
    fn parse_string_too_long() {
        let s = b"3:abcd";
        let mut parser = Parser::new();
        let err = parser.parse(s).unwrap_err();
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
        let err = parser.parse(s).unwrap_err();
        assert_eq!(Error::Eof, err);
    }

    #[test]
    fn empty_dict() {
        let s = b"de";
        let mut parser = Parser::new();
        parser.parse(s).unwrap();
        assert_eq!(&[Token::new(TokenKind::Dict, 0, 2)], &parser.tokens[..]);
    }

    #[test]
    fn unclosed_dict() {
        let s = b"d";
        let err = Parser::new().parse(s).unwrap_err();
        assert_eq!(Error::Eof, err);
    }

    #[test]
    fn key_only_dict() {
        let s = b"d1:ae";
        let err = Parser::new().parse(s).unwrap_err();
        assert_eq!(Error::Eof, err);
    }

    #[test]
    fn key_only_dict_2() {
        let s = b"d1:a1:a1:ae";
        let err = Parser::new().parse(s).unwrap_err();
        assert_eq!(Error::Eof, err);
    }

    #[test]
    fn dict_string_values() {
        let s = b"d1:a2:ab3:abc4:abcde";
        let mut parser = Parser::new();
        parser.parse(s).unwrap();
        assert_eq!(
            &[
                Token::with_size(TokenKind::Dict, 0, 20, 4, 5),
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
        parser.parse(s).unwrap();
        assert_eq!(
            &[
                Token::with_size(TokenKind::Dict, 0, 36, 12, 13),
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
        parser.parse(s).unwrap();
        assert_eq!(&[Token::new(TokenKind::List, 0, 2)], &parser.tokens[..]);
    }

    #[test]
    fn unclosed_list() {
        let s = b"l";
        let err = Parser::new().parse(s).unwrap_err();
        assert_eq!(Error::Eof, err);
    }

    #[test]
    fn list_string_values() {
        let s = b"l1:a2:ab3:abc4:abcde";
        let mut parser = Parser::new();
        parser.parse(s).unwrap();
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
        parser.parse(s).unwrap();
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
        parser.parse(s).unwrap();
        assert_eq!(
            &[
                Token::with_size(TokenKind::List, 0, 19, 1, 8),
                Token::with_size(TokenKind::Dict, 1, 18, 2, 7),
                Token::with_size(TokenKind::ByteStr, 4, 5, 0, 1),
                Token::with_size(TokenKind::List, 5, 17, 1, 5),
                Token::with_size(TokenKind::Dict, 6, 16, 2, 4),
                Token::with_size(TokenKind::ByteStr, 9, 11, 0, 1),
                Token::with_size(TokenKind::List, 11, 15, 1, 2),
                Token::with_size(TokenKind::List, 12, 14, 0, 1),
            ],
            &parser.tokens[..]
        );
    }

    #[test]
    fn token_limit() {
        let s = b"l1:a2:ab3:abc4:abcde";
        let mut parser = Parser::with_token_limit(3);
        let err = parser.parse(s).unwrap_err();
        assert_eq!(Error::NoMemory, err);
    }

    #[test]
    fn multiple_root_tokens() {
        let mut parser = Parser::new();
        assert_eq!(
            Error::Invalid {
                reason: "Extra bytes at the end",
                pos: 3,
            },
            parser.parse(b"1:a1:b").unwrap_err()
        );
        assert_eq!(
            Error::Invalid {
                reason: "Extra bytes at the end",
                pos: 3,
            },
            parser.parse(b"i1e1:b").unwrap_err()
        );
        assert_eq!(
            Error::Invalid {
                reason: "Extra bytes at the end",
                pos: 5,
            },
            parser.parse(b"l1:aede").unwrap_err()
        );
        assert_eq!(
            Error::Invalid {
                reason: "Extra bytes at the end",
                pos: 2,
            },
            parser.parse(b"lel1:ae").unwrap_err()
        );
    }

    #[test]
    fn parse_prefix() {
        let s = b"lede";
        let mut parser = Parser::new();
        let (_, len) = parser.parse_prefix(s).unwrap();
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
        let (_, len) = parser.parse_prefix(s).unwrap();
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
        parser.parse(s).unwrap();
        assert_eq!(
            &[Token::with_size(TokenKind::ByteStr, 2, 2, 0, 1)],
            &parser.tokens[..]
        );
    }
}
