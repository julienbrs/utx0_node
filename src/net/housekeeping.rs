use std::{sync::Arc, time::Duration};

use dashmap::DashMap;

use crate::{
    config::Config,
    protocol::peerlist::Peer,
    state::connection::{ConnectionOutboundMap, InboundCounter},
};

pub async fn housekeeping_loop(
    config: Arc<Config>,
    inbound: InboundCounter,
    outbound: ConnectionOutboundMap,
    peers_map: Arc<DashMap<Peer, ()>>,
) {
    loop {
        // prune finished tasks
        outbound.retain(|_, h| !h.is_finished());

        tracing::info!(
            inbound = inbound.load(),
            outbound = outbound.len(),
            peers = peers_map.len(),
            "Housekeeping status"
        );

        // // flush peer file
        // if let Err(e) = persist_peers_snapshot(&config.peers_file, &peers_map) {
        //     tracing::warn!(error = %e, "Failed to flush peers file");
        // }

        // optionally, kick the outbound loop early?
        //    (most of the time your outbound_loop is already waking up
        //     every service_loop_delay, so you can skip this.)

        tokio::time::sleep(Duration::from_secs(config.service_loop_delay)).await;
    }
}

#[cfg(test)]
mod housekeeping_tests {
    use super::*;
    use crate::{
        config::Config,
        protocol::peerlist::Peer,
        state::connection::{InboundCounter, new_outbound_map},
    };
    use dashmap::DashMap;
    use std::sync::{Arc, Once};
    use tokio::{
        task::JoinHandle,
        time::{Duration, sleep},
    };

    static INIT: Once = Once::new();
    fn init_tracing() {
        INIT.call_once(|| {
            tracing_subscriber::fmt()
                .with_max_level(tracing::Level::DEBUG)
                .with_test_writer()
                .init();
        });
    }

    #[tokio::test]
    async fn housekeeping_prunes_finished_outbound_and_logs_counts() {
        init_tracing();
        let cfg = Arc::new(Config::new_test(0, "hk/0.1", "peers.csv"));
        let peers_map = Arc::new(DashMap::new());
        let outbound = new_outbound_map();
        let inbound = InboundCounter::new();

        tokio::spawn({
            let cfg = cfg.clone();
            let om = outbound.clone();
            let pm = peers_map.clone();
            let ic = inbound.clone();
            async move { housekeeping_loop(cfg, ic, om, pm).await }
        });

        // 2 tasks, one closes, the other one sleeps
        let peer1 = Peer { host: "1.2.3.4".into(), port: 1 };
        let peer2 = Peer { host: "1.2.3.4".into(), port: 2 };

        let h1: JoinHandle<()> = tokio::spawn(async { /* ends */ });
        let h2: JoinHandle<()> = tokio::spawn(async { sleep(Duration::from_secs(60)).await });

        // ensure h1 is finished
        tokio::task::yield_now().await;

        outbound.insert(peer1.clone(), h1);
        outbound.insert(peer2.clone(), h2);

        // wait 2 iterations
        sleep(Duration::from_secs(cfg.service_loop_delay * 2)).await;

        // only h2 should be conserved
        assert_eq!(outbound.len(), 1);
        assert!(outbound.contains_key(&peer2), "h2 should survive");
        assert_eq!(inbound.load(), 0);
    }
}
