use thiserror::Error;

use crate::protocol::message::Message;

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("invalid format")] // Json not parsed/missing fields
    InvalidFormat,
    #[error("invalid handshake")] // Peers didnt send hello in time
    InvalidHandshake,
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
        }
    }
}
