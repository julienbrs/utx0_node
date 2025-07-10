use std::path::PathBuf;
const MAX_OUTBOUND: usize = 4;

#[derive(Debug)]
pub struct Config {
    pub port: u16,
    pub user_agent: String,
    pub peers_file: PathBuf,
    pub max_outbound_connection: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: 18018,
            user_agent: "utx0-v0.0".to_string(),
            peers_file: PathBuf::from("peers.csv"),
            max_outbound_connection: MAX_OUTBOUND,

        }
    }
}
