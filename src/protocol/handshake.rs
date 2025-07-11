// src/protocol/handshake.rs
use crate::{
    config::Config,
    error::ProtocolError,
    net::framing::{read_frame, write_frame},
    protocol::{message::Message, peerlist::Peer},
};
use std::time::Duration;
use tokio::{
    io::{BufReader, BufWriter, split},
    net::TcpStream,
    time::timeout,
};

/// Inbound (listener) handshake:
///  1) read Hello,
///  2) send Hello
///  3) send GetPeers
pub async fn inbound(
    socket: TcpStream,
    config: &Config,
) -> Result<
    (BufReader<tokio::io::ReadHalf<TcpStream>>, BufWriter<tokio::io::WriteHalf<TcpStream>>),
    ProtocolError,
> {
    let (r, w) = split(socket);
    let mut reader = BufReader::new(r);
    let mut writer = BufWriter::new(w);

    // 1) read Hello (with timeout)
    let hello = timeout(Duration::from_secs(config.service_loop_delay), read_frame(&mut reader))
        .await
        .map_err(|_| ProtocolError::InvalidHandshake)??;
    match hello {
        Message::Hello { port, user_agent } => {
            tracing::debug!(peer_port = port, peer_agent = %user_agent, "Received Hello");
        }
        other => {
            let err = Message::mk_error(
                "INVALID_HANDSHAKE".into(),
                format!("expected Hello, got {:?}", other),
            );
            write_frame(&mut writer, &err).await?;
            return Err(ProtocolError::InvalidHandshake);
        }
    }

    // 2) send our Hello
    let our_hello = Message::mk_hello(config.port, config.user_agent.clone());
    write_frame(&mut writer, &our_hello).await?;

    // 3) immediately send a GetPeers
    let gp = Message::mk_getpeers();
    write_frame(&mut writer, &gp).await?;

    Ok((reader, writer))
}

/// Outbound (dial) handshake:
///  1) connect + send Hello
///  2) read Hello (with timeout)
///  3) send GetPeers
pub async fn outbound(
    config: &Config,
    peer: &Peer,
) -> Result<
    (BufReader<tokio::io::ReadHalf<TcpStream>>, BufWriter<tokio::io::WriteHalf<TcpStream>>),
    ProtocolError,
> {
    tracing::info!(peer = %peer, "Dialing outbound peer");
    let stream = TcpStream::connect((peer.host.as_str(), peer.port)).await?;
    let (r, w) = split(stream);
    let mut reader = BufReader::new(r);
    let mut writer = BufWriter::new(w);

    // 1) send Hello
    let hello = Message::mk_hello(config.port, config.user_agent.clone());
    write_frame(&mut writer, &hello).await?;

    // 2) read their Hello
    let their_hello =
        timeout(Duration::from_secs(config.service_loop_delay), read_frame(&mut reader))
            .await
            .map_err(|_| ProtocolError::InvalidHandshake)??;
    match their_hello {
        Message::Hello { port, user_agent } => {
            tracing::info!(peer = %peer, %port, %user_agent, "Outbound handshake OK");
        }
        _ => return Err(ProtocolError::InvalidHandshake),
    }

    // 3) send GetPeers
    let gp = Message::mk_getpeers();
    write_frame(&mut writer, &gp).await?;

    Ok((reader, writer))
}
