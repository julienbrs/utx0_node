use std::{sync::Arc, time::Duration};

use dashmap::DashMap;
use tokio::{task::JoinHandle, time::sleep};

use crate::{
    config::Config,
    net::connection::{connect_and_handshake, run_message_loop},
    protocol::peerlist::Peer,
    state::connection::ConnectionOutboundMap,
};
use rand::seq::IteratorRandom;

const SERVICE_LOOP_DELAY: u64 = 20;

pub async fn outbound_loop(
    config: Arc<Config>,
    outbound_connections: ConnectionOutboundMap,
    peers_map: Arc<DashMap<Peer, ()>>,
) {
    loop {
        outbound_connections.retain(|_, handle| !handle.is_finished());

        let active = outbound_connections.len();
        if active < config.max_outbound_connection {
            let n_to_spawn = config.max_outbound_connection.saturating_sub(active);
            let mut rng = rand::rng();
            let candidates: Vec<_> = peers_map
                .iter()
                .map(|ent| ent.key().clone())
                .filter(|p| !outbound_connections.contains_key(p))
                .choose_multiple(&mut rng, n_to_spawn);

            for peer in candidates {
                let cfg = config.clone();
                let pm = peers_map.clone();
                let oc = outbound_connections.clone();
                let peer_for_spawn = peer.clone();
                let handle: JoinHandle<()> = tokio::spawn(async move {
                    if let Err(e) = connect_and_handshake(&cfg, &peer_for_spawn).await {
                        tracing::warn!(peer = %peer_for_spawn, error = %e, "Handshake outbound failed");
                        return;
                    }
                    if let Err(e) = run_message_loop(peer_for_spawn.clone(), pm.clone()).await {
                        tracing::warn!(peer_for_spawn = %peer_for_spawn, error = %e, "Outbound loop failed");
                    }
                    oc.remove(&peer_for_spawn);
                });
                outbound_connections.insert(peer, handle);
            }
        }

        sleep(Duration::from_secs(SERVICE_LOOP_DELAY)).await;
    }
}
