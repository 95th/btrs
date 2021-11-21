pub trait Int: sealed::DecodeInt {}

impl<T> Int for T where T: sealed::DecodeInt {}

mod sealed {
    pub trait DecodeInt: Sized {
        fn decode(buf: &[u8]) -> Option<Self>;
    }

    macro_rules! decode_signed {
        ($( $ty:ty ),+) => {
            $(
                impl DecodeInt for $ty {
                    fn decode(buf: &[u8]) -> Option<Self> {
                        let mut iter = buf.iter();
                        let mut val = 0;
                        let mut sign = 1;

                        match iter.next() {
                            Some(b'-') => sign = -1,
                            Some(c) => val = (c - b'0') as Self,
                            None => {}
                        }

                        for c in iter {
                            let d = (c - b'0') as Self;
                            val = val.checked_mul(10).and_then(|n| n.checked_add(d))?;
                        }

                        val.checked_mul(sign)
                    }
                }
            )+
        }
    }

    macro_rules! decode_unsigned {
        ($( $ty:ty ),+) => {
            $(
                impl DecodeInt for $ty {
                    fn decode(buf: &[u8]) -> Option<Self> {
                        if buf.is_empty() || buf[0] == b'-' {
                            return None;
                        }

                        let mut val: Self = 0;
                        for c in buf {
                            let d = (c - b'0') as Self;
                            val = val.checked_mul(10).and_then(|n| n.checked_add(d))?;
                        }

                        Some(val)
                    }
                }
            )+
        }
    }

    decode_signed!(i128, i64, i32, i16, i8, isize);
    decode_unsigned!(u128, u64, u32, u16, u8, usize);
}
