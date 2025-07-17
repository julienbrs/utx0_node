use std::sync::Arc;

use crate::{
    config::Config, error::ProtocolError, protocol::peerlist::Peer,
    state::connection::InboundCounter, storage::api::ObjectStore,
};
use dashmap::DashMap;

use tokio::net::{TcpListener, TcpStream};

/// Listens on `0.0.0.0:port`, accepts connections forever,
/// does inbound‐handshake + hands off into the common dispatch loop.
pub async fn start_listening(
    config: Arc<Config>,
    store: Arc<dyn ObjectStore + Send + Sync>,
    peers_map: Arc<DashMap<Peer, ()>>,
    inbound_counter: InboundCounter,
) -> Result<(), ProtocolError> {
    let addr = ("0.0.0.0", config.port);
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(port = config.port, "Listening on port {}", config.port);
    loop {
        let (socket, peer_addr) = listener.accept().await?;
        let _guard = inbound_counter.enter();
        tracing::info!(%peer_addr, "New connection");

        let cfg = config.clone();
        let peers_map = peers_map.clone();
        let ic = inbound_counter.clone();
        let store = store.clone();
        tokio::spawn(async move {
            let _guard = ic.enter();
            // `_guard` will drop (decrement) when this async task exits
            if let Err(e) = handle_connection(socket, cfg, store, peers_map).await {
                tracing::warn!(error = %e, "Connection handler failed");
            }
        });
    }
}

pub async fn handle_connection(
    socket: TcpStream,
    config: Arc<Config>,
    store: Arc<dyn ObjectStore + Send + Sync>,
    peers_map: Arc<DashMap<Peer, ()>>,
) -> Result<(), ProtocolError> {
    // do phases 1+2 of inbound handshake, then hand over to dispatch
    let (reader, writer) = crate::protocol::handshake::inbound(socket, &config).await?;
    crate::net::dispatch::run_message_loop(reader, writer, config, store, peers_map).await?;
    Ok(())
}
