use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialOrd, PartialEq)]
pub enum Value {
    Int(i64),
    Bytes(Vec<u8>),
    List(Vec<Self>),
    Dict(BTreeMap<&'static str, Self>),
}

impl Value {
    pub fn with_int(v: i64) -> Self {
        Self::Int(v)
    }

    pub fn with_str(s: &str) -> Self {
        Self::Bytes(s.as_bytes().to_vec())
    }

    pub fn with_string(s: String) -> Self {
        Self::Bytes(s.into_bytes())
    }

    pub fn with_list(list: Vec<Self>) -> Self {
        Self::List(list)
    }

    pub fn with_dict(map: BTreeMap<&'static str, Self>) -> Self {
        Self::Dict(map)
    }

    pub fn to_vec(&self) -> Vec<u8> {
        let mut v = vec![];
        self.write(&mut v);
        v
    }

    pub fn write(&self, w: &mut Vec<u8>) {
        enum Token<'a> {
            B(&'a Value),
            S(&'a str),
            E,
        }

        use Token::*;
        let mut stack = vec![B(self)];
        while !stack.is_empty() {
            match stack.pop().unwrap() {
                Token::B(v) => match v {
                    Self::Int(n) => {
                        w.push(b'i');
                        w.extend(&n.to_be_bytes());
                        w.push(b'e');
                    }
                    Self::Bytes(v) => {
                        w.extend(&v.len().to_be_bytes());
                        w.push(b':');
                        w.extend(v);
                    }
                    Self::List(v) => {
                        w.push(b'l');
                        stack.push(E);
                        stack.extend(v.iter().rev().map(|e| B(e)));
                    }
                    Self::Dict(m) => {
                        w.push(b'd');
                        stack.push(E);
                        for (k, v) in m.iter().rev() {
                            stack.push(B(v));
                            stack.push(S(k));
                        }
                    }
                },
                Token::S(s) => {
                    w.extend(&s.len().to_be_bytes());
                    w.push(b':');
                    w.extend(s.as_bytes());
                }
                Token::E => w.push(b'e'),
            }
        }
    }
}
