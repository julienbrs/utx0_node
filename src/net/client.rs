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

pub async fn outbound_loop(
    config: Arc<Config>,
    outbound_connections: ConnectionOutboundMap,
    peers_map: Arc<DashMap<Peer, ()>>,
) {
    loop {
        outbound_connections.retain(|_, handle| !handle.is_finished());
        tracing::debug!(
            active = outbound_connections.len(),
            "I am {}, Retained only active connections",
            &config.user_agent
        );

        let active = outbound_connections.len();
        if active < config.max_outbound_connection {
            let n_to_spawn = config.max_outbound_connection.saturating_sub(active);
            tracing::debug!(n_to_spawn, "Preparing to spawn outbound connections");

            let mut rng = rand::rng();
            let candidates: Vec<_> = peers_map
                .iter()
                .map(|ent| ent.key().clone())
                .filter(|p| !outbound_connections.contains_key(p))
                .choose_multiple(&mut rng, n_to_spawn);

            tracing::debug!(
                candidate_count = candidates.len(),
                "I am  {}, Selected peers for connection",
                &config.user_agent
            );

            for peer in candidates {
                tracing::debug!(%peer, "I am {}, Spawning outbound task", &config.user_agent);

                let cfg = config.clone();
                let pm = peers_map.clone();
                let oc = outbound_connections.clone();
                let peer_for_spawn = peer.clone();

                let cfg_tmp = config.clone();
                let handle: JoinHandle<()> = tokio::spawn(async move {
                    tracing::debug!(%peer_for_spawn, "I am {}, Starting handshake", &cfg_tmp.user_agent);

                    let (reader, writer) = match connect_and_handshake(&cfg, &peer_for_spawn).await
                    {
                        Ok((r, w)) => (r, w),
                        Err(e) => {
                            tracing::warn!(peer = %peer_for_spawn, error = %e, "Handshake outbound failed");
                            return;
                        }
                    };

                    tracing::debug!(%peer_for_spawn, "Handshake successful, entering message loop");

                    if let Err(e) =
                        run_message_loop((reader, writer), peer_for_spawn.clone(), pm.clone()).await
                    {
                        tracing::warn!(peer_for_spawn = %peer_for_spawn, error = %e, "Outbound loop failed");
                    }

                    tracing::debug!(%peer_for_spawn, "Removing peer from outbound connections");
                    oc.remove(&peer_for_spawn);
                });

                outbound_connections.insert(peer, handle);
            }
        }

        tracing::debug!("I am {}, sleeping for {}", &config.user_agent, &config.service_loop_delay);
        sleep(Duration::from_secs(config.service_loop_delay)).await;
    }
}
