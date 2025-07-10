use std::sync::Arc;

use dashmap::DashMap;
use tokio::task::JoinHandle;

use crate::protocol::peerlist::Peer;

pub type ConnectionOutboundMap = Arc<DashMap<Peer, JoinHandle<()>>>;

pub fn new_outbound_map() -> ConnectionOutboundMap {
    Arc::new(DashMap::new())
}

pub fn insert_outbound_connection(map: &ConnectionOutboundMap, peer: Peer, handle: JoinHandle<()>) {
    map.insert(peer, handle);
}

pub fn remove_outbound_connection(map: &ConnectionOutboundMap, peer: &Peer) {
    map.remove(peer);
}

pub fn len(map: &ConnectionOutboundMap) -> usize {
    map.len()
}
