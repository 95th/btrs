use ben::decode::List;
use ben::Parser;

fn main() {
    let s = b"l5:Hello5:Worlde";
    let mut parser = Parser::new();
    let list = parser.parse::<List>(s).unwrap();
    let mut list = list.iter();
    assert_eq!(b"Hello", list.next().unwrap().as_bytes().unwrap());
    assert_eq!(b"World", list.next().unwrap().as_bytes().unwrap());
    assert!(list.next().is_none());
}
