use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use dashmap::DashMap;
use tokio::task::JoinHandle;

use crate::protocol::peerlist::Peer;

pub type ConnectionOutboundMap = Arc<DashMap<Peer, JoinHandle<()>>>;

pub fn new_outbound_map() -> ConnectionOutboundMap {
    Arc::new(DashMap::new())
}

// pub fn insert_outbound_connection(map: &ConnectionOutboundMap, peer: Peer, handle: JoinHandle<()>) {
//     map.insert(peer, handle);
// }

// pub fn remove_outbound_connection(map: &ConnectionOutboundMap, peer: &Peer) {
//     map.remove(peer);
// }

// pub fn len(map: &ConnectionOutboundMap) -> usize {
//     map.len()
// }

#[derive(Clone)]
pub struct InboundCounter(Arc<AtomicUsize>);

impl InboundCounter {
    pub fn new() -> Self {
        InboundCounter(Arc::new(AtomicUsize::new(0)))
    }

    /// increments, returning a guard which will decrement on drop
    pub fn enter(&self) -> InboundGuard {
        self.0.fetch_add(1, Ordering::Relaxed);
        InboundGuard(self.clone())
    }

    pub fn load(&self) -> usize {
        self.0.load(Ordering::Relaxed)
    }
}

/// when this goes out of scope, the counter is decremented.
pub struct InboundGuard(InboundCounter);

impl Drop for InboundGuard {
    fn drop(&mut self) {
        self.0.0.fetch_sub(1, Ordering::Relaxed);
    }
}

impl fmt::Debug for InboundCounter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("InboundCounter").field(&self.load()).finish()
    }
}
