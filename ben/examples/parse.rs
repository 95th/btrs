use ben::decode::Dict;
use ben::Parser;

fn main() {
    let s = b"d1:a1:b1:ci1e1:x1:y1:dde1:fle1:g1:he";
    let mut parser = Parser::new();
    let dict = parser.parse::<Dict>(s).unwrap();
    println!("{:#?}", dict);
}
