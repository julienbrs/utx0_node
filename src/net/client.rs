use std::{sync::Arc, time::Duration};

use dashmap::DashMap;
use tokio::{task::JoinHandle, time::sleep};

use crate::{
    config::Config,
    net::{connection::connect_and_handshake, dispatch::run_message_loop},
    protocol::peerlist::Peer,
    state::connection::ConnectionOutboundMap,
};
use rand::seq::IteratorRandom;

// pub async fn outbound_loop(...) {
//     // Phase-0: dial + Phase-1 handshake outbound
//     // then:
//     dispatch::run_message_loop(reader, writer, config, peers_map).await
//   }

pub async fn outbound_loop(
    config: Arc<Config>,
    outbound_connections: ConnectionOutboundMap,
    peers_map: Arc<DashMap<Peer, ()>>,
) {
    loop {
        outbound_connections.retain(|_, handle| !handle.is_finished());
        tracing::debug!(
            active = outbound_connections.len(),
            "I am {}, retained only active connections",
            &config.user_agent
        );

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

                let handle: JoinHandle<()> = tokio::spawn(async move {
                    tracing::debug!(%peer_for_task, %cfg_for_task.user_agent, "Starting handshake");

                    let (reader, writer) = match connect_and_handshake(
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
                    if let Err(e) =
                        run_message_loop(reader, writer, cfg_for_task.clone(), pm_for_task.clone())
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

        tracing::debug!("I am {}, sleeping for {}s", &config.user_agent, config.service_loop_delay);
        sleep(Duration::from_secs(config.service_loop_delay)).await;
    }
}
