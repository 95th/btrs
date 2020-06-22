use ben::{Encoder, Parser};

fn main() {
    let mut v = vec![];
    let mut list = v.add_list();
    list.add(100);
    list.add("hello");

    let mut dict = list.add_dict();
    dict.add("a", &b"b"[..]);
    dict.add("x", "y");
    dict.finish();

    list.add(1);
    list.finish();

    let mut parser = Parser::new();
    let n = parser.parse(&v).unwrap();
    println!("{:#?}", n);
}
