use std::{path::Path, sync::Arc};

use redb::{Database, TableDefinition};

use crate::storage::{api::ObjectStore, error::StorageError};

static OBJECTS_TABLE: TableDefinition<String, Vec<u8>> = TableDefinition::new("objects");

pub struct RedbStore {
    db: Arc<Database>,
}

impl RedbStore {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let db = Arc::new(Database::create(path)?);
        Ok(RedbStore { db })
    }
}

impl ObjectStore for RedbStore {
    fn has(&self, object_id: &str) -> Result<bool, StorageError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(OBJECTS_TABLE)?;
        let key: String = object_id.to_string();
        Ok(table.get(key)?.is_some())
    }

    fn get(&self, object_id: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(OBJECTS_TABLE)?;
        let key: String = object_id.to_string();
        match table.get(key)? {
            Some(value_guard) => Ok(Some(value_guard.value().to_vec())),
            None => Ok(None),
        }
    }

    fn put(&self, object_id: &str, data: &[u8]) -> Result<Option<Vec<u8>>, StorageError> {
        let txn = self.db.begin_write()?;

        let old_bytes = {
            let mut table = txn.open_table(OBJECTS_TABLE)?;
            table.insert(object_id.to_string(), data.to_vec())?.map(|vg| vg.value().to_vec())
        };

        txn.commit()?;

        Ok(old_bytes)
    }
}
