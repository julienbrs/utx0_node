use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader},
};

use crate::protocol::peerlist::Peer;
use dashmap::DashMap;
use thiserror::Error;

pub type PeerMap = Arc<DashMap<Peer, ()>>;

pub fn append_peer<P: AsRef<Path>>(
    path: P,
    map: &DashMap<Peer, ()>,
    peer: &Peer,
) -> Result<(), PeersError> {
    if map.contains_key(&peer) {
        return Ok(()); // Already present
    }

    map.insert(peer.clone(), ());
    let key = format!("{},{}", peer.host, peer.port);

    let mut file = OpenOptions::new().append(true).create(true).open(path)?;
    writeln!(file, "{}", key)?;
    Ok(())
}

pub fn load_from_disk<P: AsRef<Path>>(path: P) -> PeerMap {
    let map: DashMap<Peer, ()> = DashMap::new();

    if let Ok(f) = File::open(path) {
        for line in BufReader::new(f).lines().flatten() {
            let mut parts = line.trim().split(',');
            if let (Some(host), Some(port_str)) = (parts.next(), parts.next()) {
                if let Ok(port) = port_str.parse::<u16>() {
                    let peer = Peer { host: host.trim().to_string(), port };
                    map.insert(peer, ());
                }
            }
        }
    }
    Arc::new(map)
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

// TODO: unit tests
