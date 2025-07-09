#![deny(warnings)]
#![deny(clippy::all)]
#![deny(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(dead_code)] //TODO: temporary

mod config;
mod error;
mod net;
mod protocol;
mod state;
mod util;

use std::path::PathBuf;
use std::sync::Arc;

use config::Config;
use tracing::info;
use util::logging::init_logging;

use crate::protocol::peerlist;
use crate::state::peers;

#[tokio::main]
async fn main() {
    init_logging();
    let config = Arc::new(Config::default());
    info!(config.port, "Kerma node starting up ");
    let peers_file = PathBuf::from("peers.csv");

    let peer_map = peers::load_from_disk(&peers_file);
    info!(count = peer_map.len(), "Loaded peers from disk");
    let peer = peerlist::Peer::try_from("192.168.1.1:8080").unwrap();
    peers::append_peer(&peers_file, &peer_map, &peer).unwrap();
    info!(count = peer_map.len(), "After, peers from disk");

    net::listener::start_listening(config, peer_map).await.unwrap();
}
