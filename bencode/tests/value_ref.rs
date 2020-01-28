use bencode::Error;
use bencode::ValueRef;
use std::collections::BTreeMap;

#[test]
fn encode_dict() {
    let mut m = BTreeMap::new();
    m.insert("hello", ValueRef::with_str("world"));
    let v = ValueRef::with_dict(m);
    assert_eq!("d5:hello5:worlde", v.to_string());
}

#[test]
fn to_owned() {
    let v = ValueRef::decode(b"d3:cow3:moo4:spam4:eggse").unwrap();
    assert_eq!("d3:cow3:moo4:spam4:eggse", v.to_string());
    let v = v.to_owned();
    assert_eq!("d3:cow3:moo4:spam4:eggse", v.to_string());
}

#[test]
fn decode_trailing_extra() {
    let (v, pos) = ValueRef::decode_prefix(b"d3:cow3:moo4:spam4:eggseabcd").unwrap();
    assert_eq!("d3:cow3:moo4:spam4:eggse", v.to_string());
    assert_eq!(24, pos);
}

#[test]
fn decode_none() {
    let e = ValueRef::decode_prefix(b"abcd").unwrap_err();
    assert_eq!(e, Error::InvalidChar(b'a'));
}
