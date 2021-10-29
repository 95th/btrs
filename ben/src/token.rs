use std::fmt;

#[derive(Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub start: u32,
    pub len: u32,
    pub next: u32,
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}[{};{}]", self.kind, self.start, self.len)
    }
}

impl Token {
    pub fn new(kind: TokenKind, start: u32, len: u32, next: u32) -> Self {
        Self {
            kind,
            start,
            len,
            next,
        }
    }

    pub fn finish(&mut self, pos: usize) {
        self.len = pos as u32 - self.start;
    }
}

#[derive(Debug, PartialEq, Copy, Clone)]
pub enum TokenKind {
    Dict,
    List,
    ByteStr,
    Int,
}
