use crate::metainfo::TorrentId;
use crate::tracker::AnnounceKey;
use std::path::{Path, PathBuf};
use std::time::Instant;

const CHUNK_HASH_LEN: usize = 20;

pub struct DefaultTorrent<S, F = DefaultTorrentFile>
where
    S: TorrentSource,
    F: TorrentFile,
{
    source: S,
    announce_key: Option<AnnounceKey>,
    torrent_id: TorrentId,
    name: String,
    chunk_size: usize,
    chunk_hashes: Vec<u8>,
    len: usize,
    files: Vec<F>,
    is_private: bool,
    created: Option<Instant>,
    created_by: Option<String>,
}

impl<S, F> Torrent for DefaultTorrent<S, F>
where
    S: TorrentSource,
    F: TorrentFile,
{
    type Source = S;
    type File = F;

    fn source(&self) -> &S {
        &self.source
    }

    fn announce_key(&self) -> Option<&AnnounceKey> {
        self.announce_key.as_ref()
    }

    fn torrent_id(&self) -> &TorrentId {
        &self.torrent_id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn chunk_size(&self) -> usize {
        self.chunk_size
    }

    fn chunk_hashes(&self) -> Vec<&[u8]> {
        self.chunk_hashes.chunks(CHUNK_HASH_LEN).collect()
    }

    fn len(&self) -> usize {
        self.len
    }

    fn files(&self) -> &[F] {
        &self.files
    }

    fn is_private(&self) -> bool {
        self.is_private
    }

    fn created(&self) -> Option<&Instant> {
        self.created.as_ref()
    }

    fn created_by(&self) -> Option<&str> {
        self.created_by.as_deref()
    }
}

pub trait Torrent {
    type Source: TorrentSource;
    type File: TorrentFile;

    fn source(&self) -> &Self::Source;

    /// Announce key, or `None` for trackerless torrents.
    fn announce_key(&self) -> Option<&AnnounceKey>;

    /// Torrent ID.
    fn torrent_id(&self) -> &TorrentId;

    /// Suggested name for this torrent.
    fn name(&self) -> &str;

    /// Size of a chunk, in bytes.
    fn chunk_size(&self) -> usize;

    /// Sequence of SHA-1 hashes of all chunks in this torrent.
    fn chunk_hashes(&self) -> Vec<&[u8]>;

    /// Total size of all chunks in this torrent, in bytes.
    fn len(&self) -> usize;

    /// Information on the files contained in this torrent.
    fn files(&self) -> &[Self::File];

    /// True if this torrent is private (see BEP-27)
    fn is_private(&self) -> bool;

    /// Creation time of the torrent
    fn created(&self) -> Option<&Instant>;

    /// Creator of the torrent (usually name and version of the program
    /// used to create the .torrent file)
    fn created_by(&self) -> Option<&str>;
}

pub trait TorrentSource {
    /// Returns metadata that contains all necessary information to fully
    /// re-create the torrent. Usually this means the contents of a .torrent
    /// file in BEP-3 format. It's not mandatory for normal operation.
    fn metadata(&self) -> Option<&[u8]>;

    /// Returns the part of metadata that is shared with other peers per BEP-9.
    /// Usually this means the info dictionary.
    ///
    /// Programmatically created torrents may choose to use their own metadata serialization format,
    /// given that the corresponding services (like MetadataService) are adjusted accordingly
    /// both for local and remote runtime instances.
    fn exchanged_metadata(&self) -> &[u8];
}

pub trait TorrentFile {
    /// Size of this file, in bytes.
    fn len(&self) -> usize;

    /// Path containing subdirectory names, the last of which is the actual file name
    /// (thus it always contains at least one element).
    fn path(&self) -> &Path;
}

pub struct DefaultTorrentFile {
    len: usize,
    path: PathBuf,
}

impl TorrentFile for DefaultTorrentFile {
    fn len(&self) -> usize {
        self.len
    }

    fn path(&self) -> &Path {
        &self.path
    }
}
