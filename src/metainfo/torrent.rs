use crate::metainfo::TorrentId;
use crate::tracker::AnnounceKey;
use std::path::PathBuf;
use std::time::Instant;

const CHUNK_HASH_LEN: usize = 20;

pub struct Torrent {
    source: TorrentSource,
    announce_key: Option<AnnounceKey>,
    torrent_id: TorrentId,
    name: String,
    chunk_size: usize,
    chunk_hashes: Vec<u8>,
    len: usize,
    files: Vec<TorrentFile>,
    is_private: bool,
    created: Option<Instant>,
    created_by: Option<String>,
}

impl Torrent {
    fn source(&self) -> &TorrentSource {
        &self.source
    }

    /// Announce key, or `None` for trackerless torrents.
    fn announce_key(&self) -> Option<&AnnounceKey> {
        self.announce_key.as_ref()
    }

    /// Torrent ID.
    fn torrent_id(&self) -> &TorrentId {
        &self.torrent_id
    }

    /// Suggested name for this torrent.
    fn name(&self) -> &str {
        &self.name
    }

    /// Size of a chunk, in bytes.
    fn chunk_size(&self) -> usize {
        self.chunk_size
    }

    /// Sequence of SHA-1 hashes of all chunks in this torrent.
    fn chunk_hashes(&self) -> Vec<&[u8]> {
        self.chunk_hashes.chunks(CHUNK_HASH_LEN).collect()
    }

    /// Total size of all chunks in this torrent, in bytes.
    fn len(&self) -> usize {
        self.len
    }

    /// Information on the files contained in this torrent.
    fn files(&self) -> &[TorrentFile] {
        &self.files
    }

    /// True if this torrent is private (see BEP-27)
    fn is_private(&self) -> bool {
        self.is_private
    }

    /// Creation time of the torrent
    fn created(&self) -> Option<&Instant> {
        self.created.as_ref()
    }

    /// Creator of the torrent (usually name and version of the program
    /// used to create the .torrent file)
    fn created_by(&self) -> Option<&str> {
        self.created_by.as_deref()
    }
}

pub struct TorrentSource {
    /// Returns metadata that contains all necessary information to fully
    /// re-create the torrent. Usually this means the contents of a .torrent
    /// file in BEP-3 format. It's not mandatory for normal operation.
    metadata: Option<Vec<u8>>,

    /// Returns the part of metadata that is shared with other peers per BEP-9.
    /// Usually this means the info dictionary.
    ///
    /// Programmatically created torrents may choose to use their own metadata serialization format,
    /// given that the corresponding services (like MetadataService) are adjusted accordingly
    /// both for local and remote runtime instances.
    exchanged_metadata: Vec<u8>,
}

pub struct TorrentFile {
    /// Size of this file, in bytes.
    len: usize,

    /// Path containing subdirectory names, the last of which is the actual file name
    /// (thus it always contains at least one element).
    path: PathBuf,
}
