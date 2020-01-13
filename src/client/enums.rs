pub enum TrackerState {
    Ok,
    Offline,
    InvalidResponse,
}

pub enum ListenerStatus {
    Listening,
    PortNotFree,
    NotListening,
}

pub enum Direction {
    None,
    Incoming,
    Outgoing,
}

pub enum Priority {
    DontDownload = 0,
    Lowest = 1,
    Low = 2,
    Normal = 4,
    High = 8,
    Highest = 16,
    Immediate = 32,
}

pub enum TorrentEvent {
    None,
    Started,
    Stopped,
    Completed,
}

pub enum PeerListType {
    NascentPeers,
    CandidatePeers,
    OptimisticUnchokeCandidatePeers,
}
