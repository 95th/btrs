use crate::data::{Storage, StorageUnit};
use crate::metainfo::torrent::{Torrent, TorrentFile};
use std::path::PathBuf;

pub struct FileSystemStorage {
    _root_dir: PathBuf,
}

impl Storage for FileSystemStorage {
    type Unit = FileSystemStorageUnit;

    fn get_unit(&self, _torrent: &Torrent, _file: &TorrentFile) -> Self::Unit {
        todo!()
    }
}

pub struct FileSystemStorageUnit {
    capacity: usize,
}

impl StorageUnit for FileSystemStorageUnit {
    fn capacity(&self) -> usize {
        self.capacity
    }

    fn len(&self) -> usize {
        unimplemented!()
    }
}
