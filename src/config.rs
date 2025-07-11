use std::path::PathBuf;
use thiserror::Error;

use crate::protocol::peerlist::Peer;
const DEFAULT_MAX_OUTBOUND: usize = 4;
const DEFAULT_SERVICE_LOOP_DELAY: u64 = 20;

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub user_agent: String,
    pub peers_file: PathBuf,

    pub max_outbound_connection: usize,
    pub service_loop_delay: u64, //TODO: we prob want service loop delay to be the same for every node, if so => private + getter

    pub banned_hosts: Vec<Peer>, // start empty
    pub public_node: bool,       // start "true"
    pub my_host: String,         // derive from port
}

impl Config {
    //  by reading the given toml file.
    // pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
    // }

    pub fn new(port: u16, user_agent: impl Into<String>, peers_file: impl Into<PathBuf>) -> Self {
        // we bind to 0.0.0.0:<port> for incoming, and advertise that.
        let my_host = "0.0.0.0".to_string();
        Config {
            port,
            user_agent: user_agent.into(),
            peers_file: peers_file.into(),

            max_outbound_connection: DEFAULT_MAX_OUTBOUND,
            service_loop_delay: DEFAULT_SERVICE_LOOP_DELAY,

            banned_hosts: Vec::new(),
            public_node: true,
            my_host,
        }
    }

    pub fn new_test(
        port: u16,
        user_agent: impl Into<String>,
        peers_file: impl Into<PathBuf>,
    ) -> Self {
        let my_host = "0.0.0.0".to_string();
        Config {
            port,
            user_agent: user_agent.into(),
            peers_file: peers_file.into(),

            max_outbound_connection: 4,
            service_loop_delay: 1,

            banned_hosts: Vec::new(),
            public_node: true,
            my_host,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config::new(18018, "utx0-v0.1", "peers.csv")
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("I/O error reading config file: {0}")]
    Io(#[from] std::io::Error),
    #[error("Bad peer entry: {0}")]
    BadPeer(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_config_has_defaults() {
        let cfg = Config::new(1337, "agent/1.0", "foo.csv");
        assert_eq!(cfg.port, 1337);
        assert_eq!(cfg.user_agent, "agent/1.0");
        assert_eq!(cfg.peers_file, PathBuf::from("foo.csv"));

        assert_eq!(cfg.max_outbound_connection, DEFAULT_MAX_OUTBOUND);
        assert_eq!(cfg.service_loop_delay, DEFAULT_SERVICE_LOOP_DELAY);

        assert!(cfg.banned_hosts.is_empty());
        assert!(cfg.public_node);
        assert_eq!(cfg.my_host, "0.0.0.0");
    }
}
