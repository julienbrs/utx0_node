use std::sync::Arc;

use crate::{
    config::Config,
    error::ProtocolError,
    net::framing::{read_frame, write_frame},
    protocol::message::Message,
};
use tokio::{
    io::{BufReader, BufWriter, split},
    net::{TcpListener, TcpStream},
    time::{Duration, timeout},
};

pub async fn start_listening(config: Arc<Config>) -> Result<(), ProtocolError> {
    let listener = TcpListener::bind(("0.0.0.0", config.port)).await.unwrap();
    loop {
        let (socket, peer_addr) = listener.accept().await?;
        tracing::info!(%peer_addr, "New connection");
        let config = config.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(socket, config).await {
                tracing::warn!(error = %e,  "Connection handler failed");
            }
        });
    }
}

pub async fn handle_connection(
    socket: TcpStream,
    config: Arc<Config>,
) -> Result<(), ProtocolError> {
    let (reader, writer) = split(socket);
    let mut reader = BufReader::new(reader);
    let mut writer = BufWriter::new(writer);

    let hello_rcv = timeout(Duration::from_secs(20), read_frame(&mut reader))
        .await
        .map_err(|_| ProtocolError::InvalidHandshake)??;

    match hello_rcv {
        Message::Hello { port: peer_port, user_agent: peer_agent } => {
            tracing::info!(%peer_port, %peer_agent, "Received Hello");
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

    Ok(())
}
