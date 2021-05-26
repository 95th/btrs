use itoa::Buffer;
use std::collections::BTreeMap;

/// A trait for objects that can be bencoded.
///
/// Types implementing `Encode` are able to be encoded with an instance of
/// `Encoder`.
pub trait Encode {
    /// Feed this value into given `Encoder`.
    fn encode<E: Encoder>(&self, enc: &mut E);

    /// Encode this value into a vector of bytes.
    fn encode_to_vec(&self) -> Vec<u8> {
        let mut encoder = vec![];
        self.encode(&mut encoder);
        encoder
    }
}

/// Bencode Encoder trait.
pub trait Encoder: sealed::Sealed {
    /// Encode an integer value.
    fn add_int(&mut self, value: i64);

    /// Encode a byte slice.
    fn add_bytes(&mut self, value: &[u8]);

    /// Create a new object which accepts exactly 'n' bytes lazily.
    ///
    /// The returned object's drop will panic if the total number of
    /// added bytes is not equal to 'n'.
    fn add_bytes_exact(&mut self, len: usize) -> BytesExact<'_>;

    /// Encode string slice.
    fn add_str(&mut self, value: &str);

    /// Create a new `List` in this `Encoder`.
    fn add_list(&mut self) -> List<'_>;

    /// Create a new `Dict` in this `Encoder`.
    fn add_dict(&mut self) -> Dict<'_>;

    /// Create a new `OrderedDict` in this `Encoder`.
    fn add_ordered_dict(&mut self) -> OrderedDict<'_, '_>;
}

mod sealed {
    pub trait Sealed {}

    impl Sealed for Vec<u8> {}
}

/// Add bytes lazily to given encoder.
///
/// # Panic
/// Drop will panic if the expected number of bytes
/// is not equal to actually added bytes.
pub struct BytesExact<'a> {
    buf: &'a mut Vec<u8>,
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
pub struct List<'a> {
    buf: &'a mut Vec<u8>,
}

impl List<'_> {
    /// Create a new list
    fn new(buf: &mut Vec<u8>) -> List<'_> {
        buf.push(b'l');
        List { buf }
    }

    /// `Encode` a value in this list.
    pub fn add<E: Encode>(&mut self, value: E) {
        value.encode(self.buf);
    }

    /// Create a new object which accepts exactly given number of
    /// bytes lazily.
    pub fn add_bytes_exact(&mut self, len: usize) -> BytesExact<'_> {
        self.buf.add_bytes_exact(len)
    }

    /// Create a new `List` in this list.
    pub fn add_list(&mut self) -> List<'_> {
        self.buf.add_list()
    }

    /// Create a new `Dict` in this list.
    pub fn add_dict(&mut self) -> Dict<'_> {
        self.buf.add_dict()
    }

    /// Create a new `OrderedDict` in this list.
    pub fn add_ordered_dict(&mut self) -> OrderedDict<'_, '_> {
        self.buf.add_ordered_dict()
    }

    /// Finish building this list.
    pub fn finish(self) {}
}

impl Drop for List<'_> {
    fn drop(&mut self) {
        self.buf.push(b'e');
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
pub struct Dict<'a> {
    buf: &'a mut Vec<u8>,

    #[cfg(debug_assertions)]
    last_key: Option<Vec<u8>>,
}

impl Dict<'_> {
    /// Create a new dict
    fn new(buf: &mut Vec<u8>) -> Dict<'_> {
        buf.push(b'd');
        Dict {
            buf,
            #[cfg(debug_assertions)]
            last_key: None,
        }
    }

    /// `Encode` the value for given key inside this dictionary.
    pub fn add<E: Encode>(&mut self, key: &str, value: E) {
        self.add_key(key);
        value.encode(self.buf);
    }

    /// Create a new object which accepts exactly given number of
    /// bytes lazily.
    pub fn add_bytes_exact(&mut self, key: &str, len: usize) -> BytesExact<'_> {
        self.add_key(key);
        self.buf.add_bytes_exact(len)
    }

    /// Create a new `List` for given key inside this dictionary.
    pub fn add_list(&mut self, key: &str) -> List<'_> {
        self.add_key(key);
        self.buf.add_list()
    }

    /// Create a new `Dict` for given key inside this dictionary.
    pub fn add_dict(&mut self, key: &str) -> Dict<'_> {
        self.add_key(key);
        self.buf.add_dict()
    }

    /// Create a new `OrderedDict` inside this dictionary.
    pub fn add_ordered_dict(&mut self, key: &str) -> OrderedDict<'_, '_> {
        self.add_key(key);
        self.buf.add_ordered_dict()
    }

    fn add_key(&mut self, key: &str) {
        self.assert_key_ordering(key);
        self.buf.add_str(key);
    }

    #[cfg(debug_assertions)]
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
    fn assert_key_ordering(&mut self, _key: &str) {}

    /// Finish building this dictionary.
    pub fn finish(self) {}
}

impl Drop for Dict<'_> {
    fn drop(&mut self) {
        self.buf.push(b'e');
    }
}

/// Bencode Ordered Dictionary representation.
///
/// This will maintain keys to be unique and sorted.
pub struct OrderedDict<'a, 'k> {
    buf: &'a mut Vec<u8>,
    entries: BTreeMap<&'k [u8], Vec<u8>>,
}

impl<'a, 'k> OrderedDict<'a, 'k> {
    /// Create a new dict
    fn new(buf: &'a mut Vec<u8>) -> OrderedDict<'a, 'k> {
        OrderedDict {
            buf,
            entries: BTreeMap::new(),
        }
    }

    /// Create a new `List` for given key inside this dictionary.
    pub fn add_list(&mut self, key: &'k str) -> List<'_> {
        self.add_key(key).add_list()
    }

    /// Create a new `Dict` for given key inside this dictionary.
    pub fn add_dict(&mut self, key: &'k str) -> Dict<'_> {
        self.add_key(key).add_dict()
    }

    /// Create a new `OrderedDict` inside this dictionary.
    pub fn add_ordered_dict(&mut self, key: &'k str) -> OrderedDict<'_, '_> {
        self.add_key(key).add_ordered_dict()
    }

    /// `Encode` the value for given key inside this dictionary.
    pub fn add<E: Encode>(&mut self, key: &'k str, value: E) {
        let buf = self.add_key(key);
        value.encode(buf);
    }

    fn add_key(&mut self, key: &'k str) -> &mut Vec<u8> {
        let buf = self.entries.entry(key.as_bytes()).or_insert_with(Vec::new);
        buf.clear();
        buf
    }

    /// Finish building this dictionary.
    pub fn finish(self) {}
}

impl Drop for OrderedDict<'_, '_> {
    fn drop(&mut self) {
        self.buf.push(b'd');
        for (k, v) in &self.entries {
            self.buf.add_bytes(k);
            self.buf.extend(v);
        }
        self.buf.push(b'e');
    }
}

impl Encoder for Vec<u8> {
    #[inline]
    fn add_int(&mut self, value: i64) {
        self.push(b'i');
        let mut buf = Buffer::new();
        self.extend(buf.format(value).as_bytes());
        self.push(b'e');
    }

    #[inline]
    fn add_bytes(&mut self, value: &[u8]) {
        let mut buf = Buffer::new();
        self.extend(buf.format(value.len()).as_bytes());
        self.push(b':');
        self.extend(value);
    }

    #[inline]
    fn add_bytes_exact(&mut self, len: usize) -> BytesExact<'_> {
        let mut buf = Buffer::new();
        self.extend(buf.format(len).as_bytes());
        self.push(b':');
        BytesExact {
            buf: self,
            expected: len,
            written: 0,
        }
    }

    #[inline]
    fn add_str(&mut self, value: &str) {
        self.add_bytes(value.as_bytes());
    }

    #[inline]
    fn add_list(&mut self) -> List<'_> {
        List::new(self)
    }

    #[inline]
    fn add_dict(&mut self) -> Dict<'_> {
        Dict::new(self)
    }

    #[inline]
    fn add_ordered_dict(&mut self) -> OrderedDict<'_, '_> {
        OrderedDict::new(self)
    }
}

impl<T: Encode> Encode for &T {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        (&**self).encode(enc);
    }
}

impl<T: Encode> Encode for Box<T> {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        (&**self).encode(enc);
    }
}

impl<T: Encode> Encode for Vec<T> {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        let mut list = enc.add_list();
        for t in self {
            list.add(t);
        }
        list.finish();
    }
}

impl<T: Encode> Encode for [T] {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        let mut list = enc.add_list();
        for t in self {
            list.add(t);
        }
        list.finish();
    }
}

impl Encode for &[u8] {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        enc.add_bytes(self);
    }
}

impl Encode for &str {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        enc.add_str(self);
    }
}

impl Encode for String {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        enc.add_str(self);
    }
}

impl Encode for i64 {
    fn encode<E: Encoder>(&self, enc: &mut E) {
        enc.add_int(*self);
    }
}

macro_rules! impl_arr {
    ( $($len: expr),+ ) => {
        $(
            impl Encode for [u8; $len] {
                fn encode<E: Encoder>(&self, enc: &mut E) {
                    enc.add_bytes(&self[..]);
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
        let mut e = vec![];
        e.add_int(10);
        assert_eq!(b"i10e", &e[..]);
    }

    #[test]
    fn encode_str() {
        let mut e = vec![];
        e.add_str("1000");
        assert_eq!(b"4:1000", &e[..]);
    }

    #[test]
    fn encode_dict() {
        let mut e = vec![];
        let mut dict = e.add_dict();
        dict.add("Hello", "World");
        dict.finish();
        assert_eq!(b"d5:Hello5:Worlde", &e[..]);
    }

    #[test]
    fn encode_dict_drop() {
        let mut e = vec![];
        let mut dict = e.add_dict();
        dict.add("Hello", "World");
        drop(dict);
        assert_eq!(b"d5:Hello5:Worlde", &e[..]);
    }

    #[test]
    fn encode_dict_ordered() {
        let mut e = vec![];
        let mut dict = e.add_ordered_dict();
        dict.add("b", "World");
        dict.add("a", 100);
        dict.add_list("d").add("a");
        dict.add_dict("c").add("b", "x");
        dict.finish();
        assert_eq!(&b"d1:ai100e1:b5:World1:cd1:b1:xe1:dl1:aee"[..], &e[..]);
    }

    #[test]
    fn encode_dict_ordered_drop() {
        let mut e = vec![];
        let mut dict = e.add_ordered_dict();
        dict.add("b", "World");
        dict.add("a", 100);
        dict.add_list("d").add("a");
        dict.add_dict("c").add("b", "x");
        drop(dict);
        assert_eq!(&b"d1:ai100e1:b5:World1:cd1:b1:xe1:dl1:aee"[..], &e[..]);
    }

    #[test]
    fn encode_dict_ordered_duplicate_keys() {
        let mut e = vec![];
        let mut dict = e.add_ordered_dict();
        dict.add("b", "World");
        dict.add("a", "Foo");
        dict.add("a", "Hello");
        dict.finish();
        assert_eq!(&b"d1:a5:Hello1:b5:Worlde"[..], &e[..]);
    }

    #[test]
    fn encode_list() {
        let mut e = vec![];
        let mut list = e.add_list();
        list.add("Hello");
        list.add("World");
        list.add(123);
        list.finish();
        assert_eq!(b"l5:Hello5:Worldi123ee", &e[..]);
    }

    #[test]
    fn encode_list_drop() {
        let mut e = vec![];
        let mut list = e.add_list();
        list.add("Hello");
        list.add("World");
        list.add(123);
        drop(list);
        assert_eq!(b"l5:Hello5:Worldi123ee", &e[..]);
    }

    #[test]
    fn encode_custom() {
        enum T {
            A(u8, u8),
            B { x: u32, y: &'static str },
        }

        impl Encode for T {
            fn encode<E: Encoder>(&self, encoder: &mut E) {
                match *self {
                    Self::A(a, b) => {
                        let mut dict = encoder.add_dict();
                        dict.add("0", i64::from(a));
                        dict.add("1", i64::from(b));
                    }
                    Self::B { x, y } => {
                        let mut dict = encoder.add_dict();
                        dict.add("x", i64::from(x));
                        dict.add("y", y);
                    }
                }
            }
        }

        let mut e = vec![];
        let mut list = e.add_list();
        list.add(T::A(1, 2));
        list.add(T::B {
            x: 1,
            y: "Hello world",
        });

        drop(list);
        assert_eq!(&b"ld1:0i1e1:1i2eed1:xi1e1:y11:Hello worldee"[..], &e[..]);
    }

    #[test]
    fn encode_add_bytes2_ok() {
        let mut e = vec![];
        let mut bytes = e.add_bytes_exact(4);
        bytes.add(&[0; 2]);
        bytes.add(&[0; 2]);
        drop(bytes);
        assert_eq!(&b"4:\x00\x00\x00\x00"[..], &e[..]);
    }

    #[test]
    #[should_panic]
    fn encode_add_bytes2_panic() {
        let mut e = vec![];
        let mut bytes = e.add_bytes_exact(4);
        bytes.add(&[0; 100]);
    }

    #[cfg(debug_assertions)]
    mod debug {
        use super::*;

        #[test]
        #[should_panic(expected = "Keys must be sorted")]
        fn encode_dict_unordered() {
            let mut e = vec![];
            let mut dict = e.add_dict();
            dict.add("b", "Hello");
            dict.add("a", "World");
        }

        #[test]
        #[should_panic(expected = "Keys must be unique")]
        fn encode_dict_duplicate() {
            let mut e = vec![];
            let mut dict = e.add_dict();
            dict.add("a", "Hello");
            dict.add("a", "World");
        }

        #[test]
        fn encode_dict_sorted() {
            let mut e = vec![];
            let mut dict = e.add_dict();
            dict.add("a", "Hello");
            dict.add("b", "World");
            dict.finish();
            assert_eq!(b"d1:a5:Hello1:b5:Worlde", &e[..]);
        }
    }
}
