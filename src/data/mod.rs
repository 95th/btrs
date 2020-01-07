mod fs;

use crate::metainfo::torrent::{Torrent, TorrentFile};

pub trait Storage {
    type Unit: StorageUnit;

    fn get_unit(&self, torrent: &Torrent, file: &TorrentFile) -> Self::Unit;
}

pub trait StorageUnit {
    fn capacity(&self) -> usize;

    fn len(&self) -> usize;
}
