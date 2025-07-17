use std::{sync::Arc, time::Duration};

use dashmap::DashMap;
use tokio::{task::JoinHandle, time::sleep};

use crate::{
    config::Config, net::dispatch::run_message_loop, protocol::peerlist::Peer,
    state::connection::ConnectionOutboundMap, storage::api::ObjectStore,
};
use rand::seq::IteratorRandom;

pub async fn outbound_loop(
    config: Arc<Config>,
    store: Arc<dyn ObjectStore + Send + Sync>,
    outbound_connections: ConnectionOutboundMap,
    peers_map: Arc<DashMap<Peer, ()>>,
) {
    loop {
        outbound_connections.retain(|_, handle| !handle.is_finished());

        let active = outbound_connections.len();
        if active < config.max_outbound_connection {
            let to_spawn = config.max_outbound_connection - active;
            tracing::debug!(to_spawn, "Preparing to spawn outbound connections");

            let mut rng = rand::rng();
            let candidates: Vec<_> = peers_map
                .iter()
                .map(|e| e.key().clone())
                .filter(|p| !outbound_connections.contains_key(p))
                .choose_multiple(&mut rng, to_spawn);

            for peer in candidates {
                tracing::debug!(%peer, "Spawning outbound task");

                let cfg_for_task = config.clone();
                let pm_for_task = peers_map.clone();
                let oc_for_task = outbound_connections.clone();
                let peer_for_task = peer.clone();
                let store = store.clone();

                let handle: JoinHandle<()> = tokio::spawn(async move {
                    tracing::debug!(%peer_for_task, %cfg_for_task.user_agent, "Starting handshake");

                    let (reader, writer) = match crate::protocol::handshake::outbound(
                        &cfg_for_task,
                        &peer_for_task,
                    )
                    .await
                    {
                        Ok((r, w)) => (r, w),
                        Err(e) => {
                            tracing::warn!(peer = %peer_for_task, error = %e, "Handshake failed");
                            return;
                        }
                    };

                    tracing::debug!(%peer_for_task, "Handshake OK, entering message loop");
                    if let Err(e) = run_message_loop(
                        reader,
                        writer,
                        cfg_for_task.clone(),
                        store,
                        pm_for_task.clone(),
                    )
                    .await
                    {
                        tracing::warn!(peer = %peer_for_task, error = %e, "Loop failed");
                    }

                    tracing::debug!(%peer_for_task, "Removing outbound connection");
                    oc_for_task.remove(&peer_for_task);
                });

                outbound_connections.insert(peer, handle);
            }
        }

        sleep(Duration::from_secs(config.service_loop_delay)).await;
    }
}
