use std::path::PathBuf;
const MAX_OUTBOUND: usize = 4;
const SERVICE_LOOP_DELAY: u64 = 20;

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub user_agent: String,
    pub peers_file: PathBuf,
    pub max_outbound_connection: usize,
    pub service_loop_delay: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: 18018,
            user_agent: "utx0-v0.0".to_string(),
            peers_file: PathBuf::from("peers.csv"),
            max_outbound_connection: MAX_OUTBOUND,
            service_loop_delay: SERVICE_LOOP_DELAY,
        }
    }
}
