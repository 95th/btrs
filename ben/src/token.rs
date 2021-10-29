use std::fmt;

#[derive(Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub start: u32,
    pub end: u32,
    pub next: u32,
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}[{}:{}]", self.kind, self.start, self.end)
    }
}

impl Token {
    pub fn new(kind: TokenKind, start: u32, end: u32, next: u32) -> Self {
        Self {
            kind,
            start,
            end,
            next,
        }
    }

    pub fn len(&self) -> usize {
        (self.end - self.start) as usize
    }
}

#[derive(Debug, PartialEq, Copy, Clone)]
pub enum TokenKind {
    Dict,
    List,
    ByteStr,
    Int,
}
