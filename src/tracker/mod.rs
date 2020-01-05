#[derive(Debug, Hash, PartialEq, Eq)]
pub enum AnnounceKey {
    SingleTracker(String),
    MultiTracker(Vec<Vec<String>>),
}
