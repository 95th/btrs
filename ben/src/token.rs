use std::fmt;
use std::ops::Range;

#[derive(Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub start: u32,
    pub end: u32,
    pub children: u32,
    pub next: u32,
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}[{}:{}]", self.kind, self.start, self.end)
    }
}

impl Token {
    pub fn new(kind: TokenKind, start: u32, end: u32) -> Self {
        Self::with_size(kind, start, end, 0, 1)
    }

    pub fn with_size(kind: TokenKind, start: u32, end: u32, children: u32, next: u32) -> Self {
        Self {
            kind,
            start,
            end,
            children,
            next,
        }
    }

    /// Returns this token's bounds in the original buffer.
    pub fn range(&self) -> Range<usize> {
        debug_assert!(self.end >= self.start);

        self.start as usize..self.end as usize
    }
}

#[derive(Debug, PartialEq, Copy, Clone)]
pub enum TokenKind {
    Dict,
    List,
    ByteStr,
    Int,
}
