use std::sync::Arc;

use crate::{config::Config, error::ProtocolError, protocol::peerlist::Peer};
use dashmap::DashMap;

use tokio::net::{TcpListener, TcpStream};

/// Listens on `0.0.0.0:port`, accepts connections forever,
/// does inbound‐handshake + hands off into the common dispatch loop.
pub async fn start_listening(
    config: Arc<Config>,
    peers_map: Arc<DashMap<Peer, ()>>,
) -> Result<(), ProtocolError> {
    let addr = ("0.0.0.0", config.port);
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(port = config.port, "Listening on port {}", config.port);
    loop {
        let (socket, peer_addr) = listener.accept().await?;
        tracing::info!(%peer_addr, "New connection");
        let cfg = config.clone();
        let peers_map = peers_map.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(socket, cfg, peers_map).await {
                tracing::warn!(error = %e, "Connection handler failed");
            }
        });
    }
}

pub async fn handle_connection(
    socket: TcpStream,
    config: Arc<Config>,
    peers_map: Arc<DashMap<Peer, ()>>,
) -> Result<(), ProtocolError> {
    // do phases 1+2 of inbound handshake, then hand over to dispatch
    let (reader, writer) = crate::protocol::handshake::inbound(socket, &config).await?;
    crate::net::dispatch::run_message_loop(reader, writer, config, peers_map).await?;
    Ok(())
}
