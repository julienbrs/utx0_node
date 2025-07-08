use std::{fmt, net::Ipv4Addr};
use thiserror::Error;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Peer {
    pub host: String,
    pub port: u16,
}

impl fmt::Display for Peer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.host, self.port)
    }
}

impl TryFrom<&str> for Peer {
    type Error = PeerParseError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let (host_raw, port_raw) = str::rsplit_once(&value, ':').unwrap(); // TODO: error handling

        let port: u16 = port_raw.parse()?;

        if let Ok(_) = host_raw.parse::<Ipv4Addr>() {
        } else if is_valid_dns_name(host_raw) {
        } else {
            return Err(PeerParseError::InvalidHostname(host_raw.to_string()));
        }

        Ok(Peer { host: host_raw.to_string(), port: port })
    }
}

pub fn is_valid_dns_name(_host_name: &str) -> bool {
    // TODO
    false
}

#[derive(Debug, Error)]
pub enum PeerParseError {
    #[error("missing `:` separator")]
    MissingSeparator,

    #[error("port is not a valid number")]
    InvalidPort(#[from] std::num::ParseIntError),

    #[error("port out of range (must be 0–65535)")]
    PortOutOfRange,

    #[error("invalid IPv4 address: {0}")]
    InvalidIpv4(std::net::AddrParseError),

    #[error("invalid hostname: `{0}`")]
    InvalidHostname(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_ipv4_peer_parsing() {
        let peer_str = "192.168.1.1:8080";
        let peer = Peer::try_from(peer_str).unwrap();
        assert_eq!(peer.host, "192.168.1.1");
        assert_eq!(peer.port, 8080);
        assert_eq!(peer.to_string(), "192.168.1.1:8080");
    }

    #[test]
    #[ignore]
    fn valid_dns_peer_parsing() {
        let peer_str = "node.example.com:8080";
        let peer = Peer { host: "node.example.com".into(), port: 18018 };
        assert_eq!(peer.to_string(), peer_str);
    }

    #[test]
    fn fail_missing_colon() {
        let peer_str = "192.168.1.18080";
        let res = Peer::try_from(peer_str);
        assert!(matches!(res, Err(PeerParseError::MissingSeparator)))
    }

    #[test]
    fn fail_invalid_port() {
        let res = Peer::try_from("192.168.1.1:notaport");
        assert!(matches!(res, Err(PeerParseError::InvalidPort(_))));
    }

    #[test]
    fn fail_invalid_ipv4() {
        let res = Peer::try_from("999.999.999.999:8080");
        assert!(matches!(res, Err(PeerParseError::InvalidHostname(_))));
    }

    #[test]
    #[ignore]
    fn test_invalid_hostname() {
        let res = Peer::try_from("_invalid.hostname:8080");
        assert!(matches!(res, Err(PeerParseError::InvalidHostname(_))));
    }
}
