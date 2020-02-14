use bencode::Value;
use std::collections::BTreeMap;

#[test]
fn encode_dict() {
    let mut m = BTreeMap::new();
    m.insert("hello", Value::with_str("world"));
    let v = Value::with_dict(m);
    assert_eq!(b"d5:hello5:worlde", &v.to_vec()[..]);
}

#[test]
fn encode_str() {
    let s = Value::with_str("Hello world");
    let mut w = vec![];
    s.encode(&mut w).unwrap();
    assert_eq!(w, b"11:Hello world");
}

#[test]
fn encode_i64() {
    let s = Value::with_int(100);
    let mut w = vec![];
    s.encode(&mut w).unwrap();
    assert_eq!(w, b"i100e");
}

#[test]
fn encode_list() {
    let v = Value::with_list(vec![
        Value::with_int(100),
        Value::with_str("hello"),
        Value::with_str("world"),
    ]);
    assert_eq!(b"li100e5:hello5:worlde", &v.to_vec()[..]);
}
