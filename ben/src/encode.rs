use itoa::Buffer;
use std::collections::BTreeMap;

pub struct Encoder<'a> {
    buf: &'a mut Vec<u8>,
}

impl<'a> Encoder<'a> {
    #[inline(always)]
    pub fn new(buf: &'a mut Vec<u8>) -> Self {
        Self { buf }
    }

    pub fn int(self, value: i64) {
        self.buf.push(b'i');
        let mut fmt = Buffer::new();
        self.buf.extend(fmt.format(value).as_bytes());
        self.buf.push(b'e');
    }

    pub fn bytes<I>(self, value: I)
    where
        I: AsRef<[u8]>,
    {
        let value = value.as_ref();
        let mut fmt = Buffer::new();
        self.buf.extend(fmt.format(value.len()).as_bytes());
        self.buf.push(b':');
        self.buf.extend(value);
    }

    pub fn bytes_exact(self, len: usize) -> BytesExact<'a> {
        let mut fmt = Buffer::new();
        self.buf.extend(fmt.format(len).as_bytes());
        self.buf.push(b':');
        BytesExact {
            buf: self,
            expected: len,
            written: 0,
        }
    }

    #[inline]
    pub fn list(self) -> List<'a> {
        List::new(self)
    }

    #[inline]
    pub fn dict(self) -> Dict<'a> {
        Dict::new(self)
    }

    #[inline]
    pub fn ordered_dict<'k>(self) -> OrderedDict<'a, 'k> {
        OrderedDict::new(self)
    }

    #[inline(always)]
    fn cloned(&mut self) -> Encoder<'_> {
        Encoder::new(self.buf)
    }

    #[inline(always)]
    fn push(&mut self, c: u8) {
        self.buf.push(c);
    }

    #[inline(always)]
    fn extend(&mut self, bytes: &[u8]) {
        self.buf.extend(bytes);
    }
}

/// A trait for objects that can be bencoded.
///
/// Types implementing `Encode` are able to be encoded with an instance of
/// `Encoder`.
pub trait Encode {
    /// Feed this value into given `Encoder`.
    fn encode(&self, enc: Encoder);

    /// Encode this value into a vector of bytes.
    fn encode_to_vec(&self) -> Vec<u8> {
        let mut buf = vec![];
        self.encode(Encoder::new(&mut buf));
        buf
    }
}

/// Add bytes lazily to given encoder.
///
/// # Panic
/// Drop will panic if the expected number of bytes
/// is not equal to actually added bytes.
pub struct BytesExact<'enc> {
    buf: Encoder<'enc>,
    expected: usize,
    written: usize,
}

impl BytesExact<'_> {
    /// Add given byte slice.
    pub fn add(&mut self, buf: &[u8]) {
        self.written += buf.len();
        self.buf.extend(buf);
    }

    pub fn finish(self) {}
}

impl Drop for BytesExact<'_> {
    fn drop(&mut self) {
        assert_eq!(self.expected, self.written);
    }
}

/// Bencode List representation.
pub struct List<'enc> {
    enc: Encoder<'enc>,
}

impl<'enc> List<'enc> {
    /// Create a new list
    #[inline]
    fn new(mut enc: Encoder<'enc>) -> List<'_> {
        enc.push(b'l');
        List { enc }
    }

    /// `Encode` a value in this list.
    #[inline]
    pub fn push<E: Encode>(&mut self, value: E) {
        value.encode(self.enc.cloned());
    }

    /// Create a new object which accepts exactly given number of
    /// bytes lazily.
    #[inline]
    pub fn push_bytes_exact(&mut self, len: usize) -> BytesExact<'_> {
        self.enc.cloned().bytes_exact(len)
    }

    /// Create a new `List` in this list.
    #[inline]
    pub fn push_list(&mut self) -> List<'_> {
        self.enc.cloned().list()
    }

    /// Create a new `Dict` in this list.
    #[inline]
    pub fn push_dict(&mut self) -> Dict<'_> {
        self.enc.cloned().dict()
    }

    /// Create a new `OrderedDict` in this list.
    #[inline]
    pub fn push_ordered_dict<'key>(&mut self) -> OrderedDict<'_, 'key> {
        self.enc.cloned().ordered_dict()
    }

    /// Finish building this list.
    #[inline]
    pub fn finish(self) {}
}

impl Drop for List<'_> {
    #[inline]
    fn drop(&mut self) {
        self.enc.push(b'e');
    }
}

/// Bencode Dictionary representation.
///
/// Note: This will not enforce order or uniqueness of keys.
/// These invariants have to be maintained by the caller. If the keys
/// are not known beforehand, use `OrderedDict` instead.
///
/// If the invariants don't meet in debug mode, the add calls will
/// panic.
pub struct Dict<'enc> {
    enc: Encoder<'enc>,

    #[cfg(debug_assertions)]
    last_key: Option<Vec<u8>>,
}

impl<'enc> Dict<'enc> {
    /// Create a new dict
    #[inline]
    fn new(mut enc: Encoder<'enc>) -> Dict<'enc> {
        enc.push(b'd');
        Dict {
            enc,
            #[cfg(debug_assertions)]
            last_key: None,
        }
    }

    /// `Encode` the value for given key inside this dictionary.
    #[inline]
    pub fn insert<E: Encode>(&mut self, key: &str, value: E) {
        self.insert_key(key);
        value.encode(self.enc.cloned());
    }

    /// Create a new object which accepts exactly given number of
    /// bytes lazily.
    #[inline]
    pub fn insert_bytes_exact(&mut self, key: &str, len: usize) -> BytesExact<'_> {
        self.insert_key(key);
        self.enc.cloned().bytes_exact(len)
    }

    /// Create a new `List` for given key inside this dictionary.
    #[inline]
    pub fn insert_list(&mut self, key: &str) -> List<'_> {
        self.insert_key(key);
        self.enc.cloned().list()
    }

    /// Create a new `Dict` for given key inside this dictionary.
    #[inline]
    pub fn insert_dict(&mut self, key: &str) -> Dict<'_> {
        self.insert_key(key);
        self.enc.cloned().dict()
    }

    /// Create a new `OrderedDict` inside this dictionary.
    #[inline]
    pub fn insert_ordered_dict<'key>(&mut self, key: &str) -> OrderedDict<'_, 'key> {
        self.insert_key(key);
        self.enc.cloned().ordered_dict()
    }

    fn insert_key(&mut self, key: &str) {
        self.assert_key_ordering(key);
        self.enc.cloned().bytes(key);
    }

    #[cfg(debug_assertions)]
    #[inline]
    fn assert_key_ordering(&mut self, key: &str) {
        let key = key.as_bytes();
        if let Some(last_key) = &mut self.last_key {
            if key < &last_key[..] {
                panic!("Keys must be sorted");
            }
            if key == &last_key[..] {
                panic!("Keys must be unique");
            }
            last_key.clear();
            last_key.extend(key);
        } else {
            self.last_key = Some(key.to_vec());
        }
    }

    #[cfg(not(debug_assertions))]
    #[inline]
    fn assert_key_ordering(&mut self, _key: &str) {}

    /// Finish building this dictionary.
    #[inline]
    pub fn finish(self) {}
}

impl Drop for Dict<'_> {
    #[inline]
    fn drop(&mut self) {
        self.enc.push(b'e');
    }
}

/// Bencode Ordered Dictionary representation.
///
/// This will maintain keys to be unique and sorted.
pub struct OrderedDict<'enc, 'key> {
    enc: Encoder<'enc>,
    entries: BTreeMap<&'key [u8], Vec<u8>>,
}

impl<'enc, 'key> OrderedDict<'enc, 'key> {
    /// Create a new dict
    #[inline]
    fn new(enc: Encoder<'enc>) -> OrderedDict<'enc, 'key> {
        OrderedDict {
            enc,
            entries: BTreeMap::new(),
        }
    }

    /// `Encode` the value for given key inside this dictionary.
    #[inline]
    pub fn insert<E: Encode>(&mut self, key: &'key str, value: E) {
        let mut enc = self.entry(key);
        value.encode(enc.cloned());
    }

    /// Create a new `List` for given key inside this dictionary.
    #[inline]
    pub fn insert_list(&mut self, key: &'key str) -> List<'_> {
        self.entry(key).list()
    }

    /// Create a new `Dict` for given key inside this dictionary.
    #[inline]
    pub fn insert_dict(&mut self, key: &'key str) -> Dict<'_> {
        self.entry(key).dict()
    }

    /// Create a new `OrderedDict` inside this dictionary.
    #[inline]
    pub fn insert_ordered_dict(&mut self, key: &'key str) -> OrderedDict<'_, 'key> {
        self.entry(key).ordered_dict()
    }

    fn entry(&mut self, key: &'key str) -> Encoder<'_> {
        let buf = self.entries.entry(key.as_bytes()).or_insert_with(Vec::new);
        buf.clear();
        Encoder::new(buf)
    }

    /// Finish building this dictionary.
    #[inline]
    pub fn finish(self) {}
}

impl Drop for OrderedDict<'_, '_> {
    fn drop(&mut self) {
        self.enc.push(b'd');
        for (k, v) in &self.entries {
            self.enc.cloned().bytes(k);
            self.enc.extend(v);
        }
        self.enc.push(b'e');
    }
}

impl<T: Encode> Encode for &T {
    #[inline]
    fn encode(&self, enc: Encoder) {
        (&**self).encode(enc);
    }
}

impl<T: Encode> Encode for Box<T> {
    #[inline]
    fn encode(&self, enc: Encoder) {
        (&**self).encode(enc);
    }
}

impl<T: Encode> Encode for Vec<T> {
    #[inline]
    fn encode(&self, enc: Encoder) {
        let mut list = enc.list();
        for t in self {
            list.push(t);
        }
        list.finish();
    }
}

impl<T: Encode> Encode for [T] {
    #[inline]
    fn encode(&self, enc: Encoder) {
        let mut list = enc.list();
        for t in self {
            list.push(t);
        }
        list.finish();
    }
}

impl Encode for &[u8] {
    #[inline]
    fn encode(&self, enc: Encoder) {
        enc.bytes(self);
    }
}

impl Encode for &str {
    #[inline]
    fn encode(&self, enc: Encoder) {
        enc.bytes(self);
    }
}

impl Encode for String {
    #[inline]
    fn encode(&self, enc: Encoder) {
        enc.bytes(self);
    }
}

impl Encode for i64 {
    #[inline]
    fn encode(&self, enc: Encoder) {
        enc.int(*self);
    }
}

macro_rules! impl_arr {
    ( $($len: expr),+ ) => {
        $(
            impl Encode for [u8; $len] {
                #[inline]
                fn encode(&self, enc: Encoder) {
                    enc.bytes(&self[..]);
                }
            }
        )+
    };
}

impl_arr![
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
    26, 27, 28, 29, 30, 31, 32, 64, 128, 256, 512, 1024
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_int() {
        let buf = &mut vec![];
        Encoder::new(buf).int(10);
        assert_eq!(b"i10e", &buf[..]);
    }

    #[test]
    fn encode_str() {
        let buf = &mut vec![];
        Encoder::new(buf).bytes("1000");
        assert_eq!(b"4:1000", &buf[..]);
    }

    #[test]
    fn encode_dict() {
        let buf = &mut vec![];
        let mut dict = Encoder::new(buf).dict();
        dict.insert("Hello", "World");
        dict.finish();
        assert_eq!(b"d5:Hello5:Worlde", &buf[..]);
    }

    #[test]
    fn encode_dict_drop() {
        let buf = &mut vec![];
        let mut dict = Encoder::new(buf).dict();
        dict.insert("Hello", "World");
        drop(dict);
        assert_eq!(b"d5:Hello5:Worlde", &buf[..]);
    }

    #[test]
    fn encode_dict_ordered() {
        let buf = &mut vec![];
        let mut dict = Encoder::new(buf).ordered_dict();
        dict.insert("b", "World");
        dict.insert("a", 100);
        dict.insert_list("d").push("a");
        dict.insert_dict("c").insert("b", "x");
        dict.finish();
        assert_eq!(&b"d1:ai100e1:b5:World1:cd1:b1:xe1:dl1:aee"[..], &buf[..]);
    }

    #[test]
    fn encode_dict_ordered_drop() {
        let buf = &mut vec![];
        let mut dict = Encoder::new(buf).ordered_dict();
        dict.insert("b", "World");
        dict.insert("a", 100);
        dict.insert_list("d").push("a");
        dict.insert_dict("c").insert("b", "x");
        drop(dict);
        assert_eq!(&b"d1:ai100e1:b5:World1:cd1:b1:xe1:dl1:aee"[..], &buf[..]);
    }

    #[test]
    fn encode_dict_ordered_duplicate_keys() {
        let buf = &mut vec![];
        let mut dict = Encoder::new(buf).ordered_dict();
        dict.insert("b", "World");
        dict.insert("a", "Foo");
        dict.insert("a", "Hello");
        dict.finish();
        assert_eq!(&b"d1:a5:Hello1:b5:Worlde"[..], &buf[..]);
    }

    #[test]
    fn encode_list() {
        let buf = &mut vec![];
        let mut list = Encoder::new(buf).list();
        list.push("Hello");
        list.push("World");
        list.push(123);
        list.finish();
        assert_eq!(b"l5:Hello5:Worldi123ee", &buf[..]);
    }

    #[test]
    fn encode_list_drop() {
        let buf = &mut vec![];
        let mut list = Encoder::new(buf).list();
        list.push("Hello");
        list.push("World");
        list.push(123);
        drop(list);
        assert_eq!(b"l5:Hello5:Worldi123ee", &buf[..]);
    }

    #[test]
    fn encode_custom() {
        enum T {
            A(u8, u8),
            B { x: u32, y: &'static str },
        }

        impl Encode for T {
            fn encode(&self, enc: Encoder) {
                let mut dict = enc.dict();
                match *self {
                    Self::A(a, b) => {
                        dict.insert("0", i64::from(a));
                        dict.insert("1", i64::from(b));
                    }
                    Self::B { x, y } => {
                        dict.insert("x", i64::from(x));
                        dict.insert("y", y);
                    }
                }
            }
        }

        let buf = &mut vec![];
        let mut list = Encoder::new(buf).list();
        list.push(T::A(1, 2));
        list.push(T::B {
            x: 1,
            y: "Hello world",
        });

        drop(list);
        assert_eq!(&b"ld1:0i1e1:1i2eed1:xi1e1:y11:Hello worldee"[..], &buf[..]);
    }

    #[test]
    fn encode_add_bytes2_ok() {
        let buf = &mut vec![];
        let mut bytes = Encoder::new(buf).bytes_exact(4);
        bytes.add(&[0; 2]);
        bytes.add(&[0; 2]);
        drop(bytes);
        assert_eq!(&b"4:\x00\x00\x00\x00"[..], &buf[..]);
    }

    #[test]
    #[should_panic]
    fn encode_add_bytes2_panic() {
        let buf = &mut vec![];
        let mut bytes = Encoder::new(buf).bytes_exact(4);
        bytes.add(&[0; 100]);
    }

    #[cfg(debug_assertions)]
    mod debug {
        use super::*;

        #[test]
        #[should_panic(expected = "Keys must be sorted")]
        fn encode_dict_unordered() {
            let buf = &mut vec![];
            let mut dict = Encoder::new(buf).dict();
            dict.insert("b", "Hello");
            dict.insert("a", "World");
        }

        #[test]
        #[should_panic(expected = "Keys must be unique")]
        fn encode_dict_duplicate() {
            let buf = &mut vec![];
            let mut dict = Encoder::new(buf).dict();
            dict.insert("a", "Hello");
            dict.insert("a", "World");
        }

        #[test]
        fn encode_dict_sorted() {
            let buf = &mut vec![];
            let mut dict = Encoder::new(buf).dict();
            dict.insert("a", "Hello");
            dict.insert("b", "World");
            dict.finish();
            assert_eq!(b"d1:a5:Hello1:b5:Worlde", &buf[..]);
        }
    }
}
