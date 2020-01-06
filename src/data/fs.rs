use crate::data::{Storage, StorageUnit};
use crate::metainfo::torrent::Torrent;
use std::path::PathBuf;

pub struct FileSystemStorage {
    _root_dir: PathBuf,
}

impl<T: Torrent> Storage<T> for FileSystemStorage {
    type Unit = FileSystemStorageUnit;

    fn get_unit(&self, _torrent: &T, _file: &T::File) -> Self::Unit {
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
