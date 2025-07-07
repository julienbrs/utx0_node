use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct PeerAddr {
    pub host: String,
    pub port: u16,
}
