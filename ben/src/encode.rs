use itoa::Buffer;

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
/// Types implementing `Encode` are encodable into given buffer.
pub trait Encode {
    /// Encode this value into given buffer.
    fn encode(&self, buf: &mut Vec<u8>);

    /// Encode this value into a vector of bytes.
    fn encode_to_vec(&self) -> Vec<u8> {
        let mut buf = vec![];
        self.encode(&mut buf);
        buf
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

    /// Create a new `LazyBytesEncoder` in this list.
    #[inline]
    pub fn push_bytes_lazy<const N: usize>(&mut self) -> LazyBytesEncoder<'_, N> {
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
/// These invariants have to be maintained by the caller.
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

    /// Create a new `LazyBytesEncoder` for given key inside this dictionary.
    #[inline]
    pub fn insert_bytes_lazy<const N: usize>(&mut self, key: &str) -> LazyBytesEncoder<'_, N> {
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

pub struct LazyBytesEncoder<'a, const N: usize> {
    buf: &'a mut Vec<u8>,
    data: [u8; N],
    len: usize,
}

impl<'a, const N: usize> LazyBytesEncoder<'a, N> {
    pub fn new(buf: &'a mut Vec<u8>) -> Self {
        Self {
            buf,
            data: [0; N],
            len: 0,
        }
    }

    pub fn extend(&mut self, bytes: impl AsRef<[u8]>) {
        let bytes = bytes.as_ref();
        let new_len = self.len + bytes.len();
        self.data[self.len..new_len].copy_from_slice(bytes);
        self.len = new_len;
    }

    pub fn finish(self) {}
}

impl<'a, const N: usize> Drop for LazyBytesEncoder<'a, N> {
    fn drop(&mut self) {
        self.data[..self.len].encode(self.buf);
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

impl<'a, const N: usize> From<&'a mut Vec<u8>> for LazyBytesEncoder<'a, N> {
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

impl<const N: usize> Encode for [u8; N] {
    #[inline]
    fn encode(&self, buf: &mut Vec<u8>) {
        encode_bytes(buf, self);
    }
}

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
    fn lazy_bytes_empty() {
        let mut v = vec![];
        let b = LazyBytesEncoder::<2>::new(&mut v);
        b.finish();
        assert_eq!(v, [b'0', b':']);
    }

    #[test]
    fn lazy_bytes_partially_filled() {
        let mut v = vec![];
        let mut b = LazyBytesEncoder::<2>::new(&mut v);
        b.extend(&[1]);
        b.finish();
        assert_eq!(v, [b'1', b':', 1]);
    }

    #[test]
    fn lazy_bytes_filled() {
        let mut v = vec![];
        let mut b = LazyBytesEncoder::<2>::new(&mut v);
        b.extend(&[1, 2]);
        b.finish();
        assert_eq!(v, [b'2', b':', 1, 2]);
    }

    #[test]
    #[should_panic]
    fn lazy_bytes_extra() {
        let mut v = vec![];
        let mut b = LazyBytesEncoder::<2>::new(&mut v);
        b.extend(&[1, 2, 3]);
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
