use redb::{
    CommitError, DatabaseError, StorageError as RedbStorageError, TableError, TransactionError,
};
use thiserror::Error;
#[derive(Debug, Error)]
pub enum StorageError {
    /// Underlying I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Redb database error.
    #[error("storage engine error: {0}")]
    Redb(#[from] DatabaseError),

    #[error("transaction error: {0}")]
    TransactionError(#[from] TransactionError),

    #[error("table error: {0}")]
    TableError(#[from] TableError),

    #[error("internal redb storage error: {0}")]
    RedbStorageError(#[from] RedbStorageError),

    #[error("redb commit error: {0}")]
    CommitError(#[from] CommitError),
}
