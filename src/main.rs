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
mod storage;
mod util;

use std::path::PathBuf;
use std::sync::Arc;

use config::Config;
use tracing::info;
use util::logging::init_logging;

use crate::protocol::peerlist;
use crate::state::connection::{InboundCounter, new_outbound_map};
use crate::state::peers;
use crate::storage::RedbStore;

#[tokio::main]
async fn main() {
    init_logging();
    let config = Arc::new(Config::new(18018, "utx0-v0.1", "peers.csv"));
    info!(config.port, "Kerma node starting up ");

    let store = RedbStore::new("objects.db")
        .unwrap_or_else(|e| panic!("Error during creation of redb db: {e}"));
    let store = Arc::new(store);

    let peers_file = PathBuf::from("peers.csv");
    let peer_map = peers::load_from_disk(&peers_file);
    info!(count = peer_map.len(), "Loaded peers from disk");
    let peer = peerlist::Peer::try_from("192.168.1.1:8080").unwrap();
    peers::append_peer(&peers_file, &peer_map, &peer).unwrap();
    info!(count = peer_map.len(), "After, peers registered");

    // handling outbound and inbound connections
    let inbound_counter = InboundCounter::new();
    let outbound = new_outbound_map();
    {
        let cfg2 = config.clone();
        let store = store.clone();
        let pm2 = peer_map.clone();
        let out2 = outbound.clone();
        tokio::spawn(async move {
            net::client::outbound_loop(cfg2, store, out2, pm2).await;
        });
    }

    // housekeeping
    {
        let cfg = config.clone();
        let ic = inbound_counter.clone();
        let om = outbound.clone();
        let pm = peer_map.clone();
        tokio::spawn(async move {
            net::housekeeping::housekeeping_loop(cfg, ic, om, pm).await;
        });
    }

    net::listener::start_listening(config, store.clone(), peer_map, inbound_counter).await.unwrap();
}
