use itoa::Buffer;
use std::collections::BTreeMap;

pub fn encode_int(buf: &mut Vec<u8>, value: i64) {
    buf.push(b'i');
    let mut fmt = Buffer::new();
    buf.extend(fmt.format(value).as_bytes());
    buf.push(b'e');
}

pub fn encode_bytes<I>(buf: &mut Vec<u8>, value: I)
where
    I: AsRef<[u8]>,
{
    let value = value.as_ref();
    let mut fmt = Buffer::new();
    buf.extend(fmt.format(value.len()).as_bytes());
    buf.push(b':');
    buf.extend(value);
}

/// A trait for objects that can be bencoded.
///
/// Types implementing `Encode` are able to be encoded with an instance of
/// `Encoder`.
pub trait Encode {
    /// Feed this value into given `Encoder`.
    fn encode(&self, buf: &mut Vec<u8>);

    /// Encode this value into a vector of bytes.
    fn encode_to_vec(&self) -> Vec<u8> {
        let mut buf = vec![];
        self.encode(&mut buf);
        buf
    }
}

/// Add bytes lazily to given encoder.
///
/// # Panic
/// Drop will panic if the expected number of bytes
/// is not equal to actually added bytes.
pub struct ExactBytesEncoder<'a> {
    buf: &'a mut Vec<u8>,
    expected: usize,
    written: usize,
}

impl<'a> ExactBytesEncoder<'a> {
    pub fn new(buf: &'a mut Vec<u8>, len: usize) -> Self {
        let mut fmt = Buffer::new();
        buf.extend(fmt.format(len).as_bytes());
        buf.push(b':');
        Self {
            buf,
            expected: len,
            written: 0,
        }
    }

    /// Add given byte slice.
    pub fn add(&mut self, buf: &[u8]) {
        self.written += buf.len();
        self.buf.extend(buf);
    }

    pub fn finish(self) {}
}

impl Drop for ExactBytesEncoder<'_> {
    fn drop(&mut self) {
        assert_eq!(self.expected, self.written);
    }
}

/// Bencode List representation.
pub struct ListEncoder<'a> {
    buf: &'a mut Vec<u8>,
}

impl<'a> ListEncoder<'a> {
    /// Create a new list
    #[inline]
    pub fn new(buf: &'a mut Vec<u8>) -> Self {
        buf.push(b'l');
        Self { buf }
    }

    /// `Encode` a value in this list.
    #[inline]
    pub fn push<E: Encode>(&mut self, value: E) {
        value.encode(self.buf);
    }

    /// Create a new object which accepts exactly given number of
    /// bytes lazily.
    #[inline]
    pub fn push_bytes_exact(&mut self, len: usize) -> ExactBytesEncoder<'_> {
        ExactBytesEncoder::new(self.buf, len)
    }

    /// Create a new `ListEncoder` in this list.
    #[inline]
    pub fn push_list(&mut self) -> ListEncoder<'_> {
        self.buf.into()
    }

    /// Create a new `DictEncoder` in this list.
    #[inline]
    pub fn push_dict(&mut self) -> DictEncoder<'_> {
        self.buf.into()
    }

    /// Create a new `SortedDictEncoder` in this list.
    #[inline]
    pub fn push_sorted_dict<'key>(&mut self) -> SortedDictEncoder<'_, 'key> {
        self.buf.into()
    }

    /// Finish building this list.
    #[inline]
    pub fn finish(self) {}
}

impl Drop for ListEncoder<'_> {
    #[inline]
    fn drop(&mut self) {
        self.buf.push(b'e');
    }
}

/// Bencode Dictionary representation.
///
/// Note: This will not enforce order or uniqueness of keys.
/// These invariants have to be maintained by the caller. If the keys
/// are not known beforehand, use `SortedDictEncoder` instead.
///
/// If the invariants don't meet in debug mode, the add calls will
/// panic.
pub struct DictEncoder<'a> {
    buf: &'a mut Vec<u8>,

    #[cfg(debug_assertions)]
    last_key: Option<Vec<u8>>,
}

impl<'a> DictEncoder<'a> {
    /// Create a new dict
    #[inline]
    pub fn new(buf: &'a mut Vec<u8>) -> Self {
        buf.push(b'd');
        Self {
            buf,
            #[cfg(debug_assertions)]
            last_key: None,
        }
    }

    /// `Encode` the value for given key inside this dictionary.
    #[inline]
    pub fn insert<E: Encode>(&mut self, key: &str, value: E) {
        self.insert_key(key);
        value.encode(self.buf);
    }

    /// Create a new object which accepts exactly given number of
    /// bytes lazily.
    #[inline]
    pub fn insert_bytes_exact(&mut self, key: &str, len: usize) -> ExactBytesEncoder<'_> {
        self.insert_key(key);
        ExactBytesEncoder::new(self.buf, len)
    }

    /// Create a new `ListEncoder` for given key inside this dictionary.
    #[inline]
    pub fn insert_list(&mut self, key: &str) -> ListEncoder<'_> {
        self.insert_key(key);
        self.buf.into()
    }

    /// Create a new `DictEncoder` for given key inside this dictionary.
    #[inline]
    pub fn insert_dict(&mut self, key: &str) -> DictEncoder<'_> {
        self.insert_key(key);
        self.buf.into()
    }

    /// Create a new `SortedDictEncoder` inside this dictionary.
    #[inline]
    pub fn insert_sorted_dict<'key>(&mut self, key: &str) -> SortedDictEncoder<'_, 'key> {
        self.insert_key(key);
        self.buf.into()
    }

    fn insert_key(&mut self, key: &str) {
        self.assert_key_ordering(key);
        encode_bytes(self.buf, key);
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

impl Drop for DictEncoder<'_> {
    #[inline]
    fn drop(&mut self) {
        self.buf.push(b'e');
    }
}

/// Bencode Ordered Dictionary representation.
///
/// This will maintain keys to be unique and sorted.
pub struct SortedDictEncoder<'buf, 'key> {
    buf: &'buf mut Vec<u8>,
    entries: BTreeMap<&'key str, Vec<u8>>,
}

impl<'buf, 'key> SortedDictEncoder<'buf, 'key> {
    /// Create a new dict
    #[inline]
    pub fn new(buf: &'buf mut Vec<u8>) -> Self {
        Self {
            buf,
            entries: BTreeMap::new(),
        }
    }

    /// `Encode` the value for given key inside this dictionary.
    #[inline]
    pub fn insert<E: Encode>(&mut self, key: &'key str, value: E) {
        value.encode(self.entry(key));
    }

    /// Create a new `ListEncoder` for given key inside this dictionary.
    #[inline]
    pub fn insert_list(&mut self, key: &'key str) -> ListEncoder<'_> {
        self.entry(key).into()
    }

    /// Create a new `DictEncoder` for given key inside this dictionary.
    #[inline]
    pub fn insert_dict(&mut self, key: &'key str) -> DictEncoder<'_> {
        self.entry(key).into()
    }

    /// Create a new `SortedDictEncoder` inside this dictionary.
    #[inline]
    pub fn insert_sorted_dict(&mut self, key: &'key str) -> SortedDictEncoder<'_, 'key> {
        self.entry(key).into()
    }

    fn entry(&mut self, key: &'key str) -> &mut Vec<u8> {
        let buf = self.entries.entry(key).or_insert_with(Vec::new);
        buf.clear();
        buf
    }

    /// Finish building this dictionary.
    #[inline]
    pub fn finish(self) {}
}

impl Drop for SortedDictEncoder<'_, '_> {
    fn drop(&mut self) {
        self.buf.push(b'd');
        for (k, v) in &self.entries {
            encode_bytes(self.buf, k);
            self.buf.extend(v);
        }
        self.buf.push(b'e');
    }
}

impl<'a> From<&'a mut Vec<u8>> for ListEncoder<'a> {
    fn from(buf: &'a mut Vec<u8>) -> Self {
        Self::new(buf)
    }
}

impl<'a> From<&'a mut Vec<u8>> for DictEncoder<'a> {
    fn from(buf: &'a mut Vec<u8>) -> Self {
        Self::new(buf)
    }
}

impl<'a> From<&'a mut Vec<u8>> for SortedDictEncoder<'a, '_> {
    fn from(buf: &'a mut Vec<u8>) -> Self {
        Self::new(buf)
    }
}

impl<T: Encode + ?Sized> Encode for &T {
    #[inline]
    fn encode(&self, buf: &mut Vec<u8>) {
        (&**self).encode(buf);
    }
}

impl<T: Encode + ?Sized> Encode for Box<T> {
    #[inline]
    fn encode(&self, buf: &mut Vec<u8>) {
        (&**self).encode(buf);
    }
}

impl<T: Encode> Encode for Vec<T> {
    #[inline]
    fn encode(&self, buf: &mut Vec<u8>) {
        let mut list = ListEncoder::new(buf);
        for t in self {
            list.push(t);
        }
        list.finish();
    }
}

impl<T: Encode> Encode for [T] {
    #[inline]
    fn encode(&self, buf: &mut Vec<u8>) {
        let mut list = ListEncoder::new(buf);
        for t in self {
            list.push(t);
        }
        list.finish();
    }
}

impl Encode for [u8] {
    #[inline]
    fn encode(&self, buf: &mut Vec<u8>) {
        encode_bytes(buf, self);
    }
}

impl Encode for str {
    #[inline]
    fn encode(&self, buf: &mut Vec<u8>) {
        encode_bytes(buf, self);
    }
}

impl Encode for String {
    #[inline]
    fn encode(&self, buf: &mut Vec<u8>) {
        encode_bytes(buf, self);
    }
}

impl Encode for i64 {
    #[inline]
    fn encode(&self, buf: &mut Vec<u8>) {
        encode_int(buf, *self);
    }
}

macro_rules! impl_arr {
    ( $($len: expr),+ ) => {
        $(
            impl Encode for [u8; $len] {
                #[inline]
                fn encode(&self, buf: &mut Vec<u8>) {
                    encode_bytes(buf, &self[..]);
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
    fn encode_integer() {
        let buf = &mut vec![];
        encode_int(buf, 10);
        assert_eq!(b"i10e", &buf[..]);
    }

    #[test]
    fn encode_str() {
        let buf = &mut vec![];
        encode_bytes(buf, "1000");
        assert_eq!(b"4:1000", &buf[..]);
    }

    #[test]
    fn encode_dict() {
        let buf = &mut vec![];
        let mut dict = DictEncoder::new(buf);
        dict.insert("Hello", "World");
        dict.finish();
        assert_eq!(b"d5:Hello5:Worlde", &buf[..]);
    }

    #[test]
    fn encode_dict_drop() {
        let buf = &mut vec![];
        let mut dict = DictEncoder::new(buf);
        dict.insert("Hello", "World");
        drop(dict);
        assert_eq!(b"d5:Hello5:Worlde", &buf[..]);
    }

    #[test]
    fn encode_dict_sorted() {
        let buf = &mut vec![];
        let mut dict = SortedDictEncoder::new(buf);
        dict.insert("b", "World");
        dict.insert("a", 100);
        dict.insert_list("d").push("a");
        dict.insert_dict("c").insert("b", "x");
        dict.finish();
        assert_eq!(&b"d1:ai100e1:b5:World1:cd1:b1:xe1:dl1:aee"[..], &buf[..]);
    }

    #[test]
    fn encode_dict_sorted_drop() {
        let buf = &mut vec![];
        let mut dict = SortedDictEncoder::new(buf);
        dict.insert("b", "World");
        dict.insert("a", 100);
        dict.insert_list("d").push("a");
        dict.insert_dict("c").insert("b", "x");
        drop(dict);
        assert_eq!(&b"d1:ai100e1:b5:World1:cd1:b1:xe1:dl1:aee"[..], &buf[..]);
    }

    #[test]
    fn encode_dict_sorted_duplicate_keys() {
        let buf = &mut vec![];
        let mut dict = SortedDictEncoder::new(buf);
        dict.insert("b", "World");
        dict.insert("a", "Foo");
        dict.insert("a", "Hello");
        dict.finish();
        assert_eq!(&b"d1:a5:Hello1:b5:Worlde"[..], &buf[..]);
    }

    #[test]
    fn encode_list() {
        let buf = &mut vec![];
        let mut list = ListEncoder::new(buf);
        list.push("Hello");
        list.push("World");
        list.push(123);
        list.finish();
        assert_eq!(b"l5:Hello5:Worldi123ee", &buf[..]);
    }

    #[test]
    fn encode_list_drop() {
        let buf = &mut vec![];
        let mut list = ListEncoder::new(buf);
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
            fn encode(&self, buf: &mut Vec<u8>) {
                let mut dict = DictEncoder::new(buf);
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
        let mut list = ListEncoder::new(buf);
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
        let mut bytes = ExactBytesEncoder::new(buf, 4);
        bytes.add(&[0; 2]);
        bytes.add(&[0; 2]);
        drop(bytes);
        assert_eq!(&b"4:\x00\x00\x00\x00"[..], &buf[..]);
    }

    #[test]
    #[should_panic]
    fn encode_add_bytes2_panic() {
        let buf = &mut vec![];
        let mut bytes = ExactBytesEncoder::new(buf, 4);
        bytes.add(&[0; 100]);
    }

    #[cfg(debug_assertions)]
    mod debug {
        use super::*;

        #[test]
        #[should_panic(expected = "Keys must be sorted")]
        fn encode_dict_unordered() {
            let buf = &mut vec![];
            let mut dict = DictEncoder::new(buf);
            dict.insert("b", "Hello");
            dict.insert("a", "World");
        }

        #[test]
        #[should_panic(expected = "Keys must be unique")]
        fn encode_dict_duplicate() {
            let buf = &mut vec![];
            let mut dict = DictEncoder::new(buf);
            dict.insert("a", "Hello");
            dict.insert("a", "World");
        }

        #[test]
        fn encode_dict_sorted() {
            let buf = &mut vec![];
            let mut dict = DictEncoder::new(buf);
            dict.insert("a", "Hello");
            dict.insert("b", "World");
            dict.finish();
            assert_eq!(b"d1:a5:Hello1:b5:Worlde", &buf[..]);
        }
    }
}
