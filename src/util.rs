use std::convert::TryInto;

/// Read a `u32` value from given slice in Big Endian order.
///
/// # Panics:
/// If slice length is not 4 bytes.
pub fn read_u32(buf: &[u8]) -> u32 {
    let b = buf.try_into().unwrap();
    u32::from_be_bytes(b)
}
