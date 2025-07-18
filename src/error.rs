use thiserror::Error;

use crate::protocol::message::Message;

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("invalid format")] // Json not parsed/missing fields
    InvalidFormat,
    #[error("invalid handshake")] // Peers didnt send hello in time
    InvalidHandshake,
    #[error("I/O error: {0}")]
    Io(std::io::Error),
    #[error("connection closed")]
    ConnectionClosed,
    #[error("frame too large (max 512 KiB)")]
    OversizedFrame,
    #[error("storage error: {0}")]
    Storage(#[from] crate::storage::error::StorageError),
}

impl From<std::io::Error> for ProtocolError {
    fn from(e: std::io::Error) -> Self {
        ProtocolError::Io(e)
    }
}

impl From<ProtocolError> for Message {
    fn from(e: ProtocolError) -> Self {
        match e {
            ProtocolError::InvalidFormat => {
                Message::mk_error("INVALID_FORMAT".to_string(), e.to_string())
            }
            ProtocolError::InvalidHandshake => {
                Message::mk_error("INVALID_HANDSHAKE".to_string(), e.to_string())
            }
            ProtocolError::ConnectionClosed => {
                Message::mk_error("CONNNECTION_CLOSED".to_string(), e.to_string())
            }
            ProtocolError::Io(_) => Message::mk_error("IO_ERROR".to_string(), e.to_string()),
            ProtocolError::OversizedFrame => {
                Message::mk_error("OVERSIZED_FRAME".to_string(), e.to_string())
            }
            ProtocolError::Storage(inner) => {
                Message::mk_error("INTERNAL_STORAGE_ERROR".into(), inner.to_string())
            }
        }
    }
}
