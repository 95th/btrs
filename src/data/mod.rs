mod fs;

use crate::metainfo::torrent::Torrent;

pub trait Storage<T: Torrent> {
    type Unit: StorageUnit;

    fn get_unit(&self, torrent: &T, file: &T::File) -> Self::Unit;
}

pub trait StorageUnit {
    fn capacity(&self) -> usize;

    fn len(&self) -> usize;
}
