/// AnnounceKey encapsulates information about trackers,
/// that can be used for getting a particular torrent.
#[derive(Debug, Hash, PartialEq, Eq)]
pub enum AnnounceKey {
    /// A single-tracker announce key
    SingleTracker(String),

    /// A multi-tracker announce key containing list of tiers of trackers.
    /// First list contains primary trackers, subsequent lists contain backup trackers.
    ///
    /// See BEP-12: Multitracker Metadata Extension for more details.
    ///
    MultiTracker(Vec<Vec<String>>),
}

impl AnnounceKey {
    pub fn is_multi_key(&self) -> bool {
        matches!(self, Self::MultiTracker(_))
    }

    pub fn tracker_url(&self) -> Option<&str> {
        if_matches!(self, Self::SingleTracker(t), t)
    }

    pub fn tracker_urls(&self) -> Option<&[Vec<String>]> {
        if_matches!(self, Self::MultiTracker(v), v)
    }
}
