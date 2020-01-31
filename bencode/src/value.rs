use std::collections::BTreeMap;
use std::fmt;
use std::io;

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

    impl_is_ty! {
        is_string == Bytes,
        is_int == Int,
        is_list == List,
        is_dict == Dict,
    }

    pub fn as_int(&self) -> Option<i64> {
        inner_if!(self == Int).copied()
    }

    pub fn as_str(&self) -> Option<&str> {
        inner_if!(self == Bytes).and_then(|buf| std::str::from_utf8(buf).ok())
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        inner_if!(self == Bytes)
    }

    pub fn as_list(&self) -> Option<&[Self]> {
        inner_if!(self == List)
    }

    pub fn as_list_mut(&mut self) -> Option<&mut Vec<Self>> {
        inner_if!(self == List)
    }

    pub fn into_list(self) -> Option<Vec<Self>> {
        inner_if!(self == List)
    }

    pub fn as_dict(&self) -> Option<&BTreeMap<&'static str, Self>> {
        inner_if!(self == Dict)
    }

    pub fn as_dict_mut(&mut self) -> Option<&mut BTreeMap<&'static str, Self>> {
        inner_if!(self == Dict)
    }

    pub fn into_dict(self) -> Option<BTreeMap<&'static str, Self>> {
        inner_if!(self == Dict)
    }

    pub fn encode_to_vec(&self) -> Vec<u8> {
        let mut v = vec![];
        self.encode(&mut v).unwrap();
        v
    }

    pub fn dict_find(&self, key: &str) -> Option<&Self> {
        let dict = self.as_dict()?;
        dict.get(key)
    }

    pub fn dict_find_int(&self, key: &str) -> Option<&Self> {
        let n = self.dict_find(key)?;
        if n.is_int() {
            Some(n)
        } else {
            None
        }
    }

    pub fn dict_find_int_value(&self, key: &str) -> Option<i64> {
        self.dict_find_int(key)?.as_int()
    }

    pub fn dict_find_str(&self, key: &str) -> Option<&Self> {
        let n = self.dict_find(key)?;
        if n.is_string() {
            Some(n)
        } else {
            None
        }
    }

    pub fn dict_find_str_value(&self, key: &str) -> Option<&str> {
        self.dict_find_str(key)?.as_str()
    }

    pub fn dict_find_list(&self, key: &str) -> Option<&Self> {
        let n = self.dict_find(key)?;
        if n.is_list() {
            Some(n)
        } else {
            None
        }
    }

    pub fn dict_find_list_value(&self, key: &str) -> Option<&[Self]> {
        self.dict_find_list(key)?.as_list()
    }

    pub fn dict_find_dict(&self, key: &str) -> Option<&Self> {
        let n = self.dict_find(key)?;
        if n.is_dict() {
            Some(n)
        } else {
            None
        }
    }

    pub fn dict_len(&self) -> Option<usize> {
        Some(self.as_dict()?.len())
    }

    pub fn list_at(&self, index: usize) -> Option<&Self> {
        let list = self.as_list()?;
        list.get(index)
    }

    pub fn list_string_value_at(&self, index: usize) -> Option<&str> {
        self.list_at(index)?.as_str()
    }

    pub fn list_int_value_at(&self, index: usize) -> Option<i64> {
        self.list_at(index)?.as_int()
    }

    pub fn list_len(&self) -> Option<usize> {
        Some(self.as_list()?.len())
    }

    pub fn encode<W: io::Write>(&self, w: &mut W) -> io::Result<()> {
        enum Token<'a> {
            B(&'a Value),
            S(&'a str),
            E,
        }

        use Token::*;
        let mut stack = vec![B(self)];
        while !stack.is_empty() {
            match stack.pop().unwrap() {
                B(v) => match v {
                    Self::Int(n) => {
                        write!(w, "i{}e", n)?;
                    }
                    Self::Bytes(v) => {
                        write!(w, "{}:", v.len())?;
                        w.write_all(&v)?;
                    }
                    Self::List(v) => {
                        write!(w, "l")?;
                        stack.push(E);
                        stack.extend(v.iter().rev().map(|e| B(e)));
                    }
                    Self::Dict(m) => {
                        write!(w, "d")?;
                        stack.push(E);
                        for (k, v) in m.iter().rev() {
                            stack.push(B(v));
                            stack.push(S(k));
                        }
                    }
                },
                S(s) => {
                    write!(w, "{}:{}", s.len(), s)?;
                }
                E => {
                    write!(w, "e")?;
                }
            }
        }
        Ok(())
    }
}

impl From<&[u8]> for Value {
    fn from(value: &[u8]) -> Self {
        Self::Bytes(value.to_vec())
    }
}

impl From<Vec<u8>> for Value {
    fn from(value: Vec<u8>) -> Self {
        Self::Bytes(value)
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", String::from_utf8_lossy(&self.encode_to_vec()))
    }
}
