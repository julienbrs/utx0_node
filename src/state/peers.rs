use std::io::Write;
use std::{
    collections::HashSet,
    fs::{File, OpenOptions},
    io::{BufRead, BufReader},
};

use crate::protocol::peerlist::Peer;
use thiserror::Error;

fn append_peer(peer: &Peer) -> Result<(), PeersError> {
    let peers = load_peers()?;
    let key = format!("{},{}", peer.host, peer.port);

    let mut file = OpenOptions::new().append(true).create(true).open("peers.csv")?;
    if peers.contains(&key) {
        return Ok(()); // Already present → skip
    }

    writeln!(file, "{}", key)?;
    Ok(())
}

fn load_peers() -> Result<HashSet<String>, PeersError> {
    let mut seen = HashSet::new();

    for line in BufReader::new(File::open("peers.csv")?).lines() {
        if let Ok(line) = line {
            seen.insert(line);
        }
    }
    return Ok(seen);
}

#[derive(Error, Debug)]
pub enum PeersError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),

    #[error("Invalid peer format")]
    InvalidPeer,
}
