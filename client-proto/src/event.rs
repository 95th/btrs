#[derive(Debug, PartialEq)]
pub enum Event {
    Metadata(Vec<u8>),
}
