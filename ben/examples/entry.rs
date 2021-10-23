use ben::decode::List;
use ben::{Encoder, Parser};

fn main() {
    let mut v = vec![];
    let mut list = Encoder::new(&mut v).list();
    list.push(100);
    list.push("hello");

    let mut dict = list.push_dict();
    dict.insert("a", &b"b"[..]);
    dict.insert("x", "y");
    dict.finish();

    list.push(1);
    list.finish();

    let mut parser = Parser::new();
    let n = parser.parse::<List>(&v).unwrap();
    println!("{:#?}", n);
}
