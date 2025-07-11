use std::sync::Arc;

use crate::{
    config::Config,
    error::ProtocolError,
    net::framing::{read_frame, write_frame},
    protocol::{message::Message, peerlist::Peer},
};
use dashmap::DashMap;

use tokio::{
    io::{BufReader, BufWriter, split},
    net::{TcpListener, TcpStream},
    time::{Duration, timeout},
};

pub async fn serve(
    listener: TcpListener,
    config: Arc<Config>,
    peers_map: Arc<DashMap<Peer, ()>>,
) -> Result<(), ProtocolError> {
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

pub async fn start_listening(
    config: Arc<Config>,
    peers_map: Arc<DashMap<Peer, ()>>,
) -> Result<(), ProtocolError> {
    let addr = ("0.0.0.0", config.port);
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(port = config.port, "Listening on port {}", config.port);
    serve(listener, config, peers_map).await
}

pub async fn handle_connection(
    socket: TcpStream,
    config: Arc<Config>,
    peers_map: Arc<DashMap<Peer, ()>>,
) -> Result<(), ProtocolError> {
    let (reader, writer) = split(socket);
    let mut reader = BufReader::new(reader);
    let mut writer = BufWriter::new(writer);

    let hello_rcv = timeout(Duration::from_secs(20), read_frame(&mut reader))
        .await
        .map_err(|_| ProtocolError::InvalidHandshake)??;

    match hello_rcv {
        Message::Hello { port: peer_port, user_agent: peer_agent } => {
            tracing::debug!(%peer_port, %peer_agent, "I am {}, Received Hello", &config.user_agent);
        }
        other => {
            // send an error and bail out
            let err = Message::mk_error(
                "INVALID_HANDSHAKE".into(),
                format!("expected Hello, got {:?}", other),
            );
            write_frame(&mut writer, &err).await?;
            return Err(ProtocolError::InvalidHandshake);
        }
    }

    let our_hello = Message::mk_hello(config.port, config.user_agent.clone());
    write_frame(&mut writer, &our_hello).await?;

    let gp = Message::mk_getpeers();
    write_frame(&mut writer, &gp).await?;

    crate::net::dispatch::run_message_loop(reader, writer, config, peers_map).await?;
    Ok(())
}
