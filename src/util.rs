use std::ascii::escape_default;

pub fn ascii_escape(buf: &[u8]) -> String {
    let v = buf.iter().flat_map(|&c| escape_default(c)).collect();

    // Safety: output of escape_default is valid UTF-8
    unsafe { String::from_utf8_unchecked(v) }
}
