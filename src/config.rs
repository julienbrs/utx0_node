use std::path::PathBuf;

#[derive(Debug)]
pub struct Config {
    pub port: u16,
    pub user_agent: String,
    pub peers_file: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: 18018,
            user_agent: "utx0-v0.0".to_string(),
            peers_file: PathBuf::from("peers.csv"),
        }
    }
}
