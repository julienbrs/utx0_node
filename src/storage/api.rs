use crate::storage::error::StorageError;

/// A simple content-addressable blob store.
pub trait ObjectStore {
    fn has(&self, object_id: &str) -> Result<bool, StorageError>;

    fn get(&self, object_id: &str) -> Result<Option<Vec<u8>>, StorageError>;

    fn put(&self, object_id: &str, data: &[u8]) -> Result<Option<Vec<u8>>, StorageError>;
}
