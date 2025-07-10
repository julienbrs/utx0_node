use std::{sync::Arc, time::Duration};

use dashmap::DashMap;
use tokio::{
    io::{BufReader, BufWriter, split},
    net::TcpStream,
};

use crate::{
    config::Config,
    error::ProtocolError,
    net::framing::{read_frame, write_frame},
    protocol::{message::Message, peerlist::Peer},
};

const TIMEOUT_HELLO: u64 = 20;

pub async fn connect_and_handshake(config: &Config, peer: &Peer) -> Result<(), ProtocolError> {
    tracing::info!(peer = %peer, "Dialing outbound peer");
    let stream = TcpStream::connect((peer.host.as_str(), peer.port)).await?;
    let (r, w) = split(stream);
    let mut reader = BufReader::new(r);
    let mut writer = BufWriter::new(w);

    let hello = Message::mk_hello(config.port, config.user_agent.clone());
    write_frame(&mut writer, &hello).await?;

    let their_hello =
        tokio::time::timeout(Duration::from_secs(TIMEOUT_HELLO), read_frame(&mut reader))
            .await
            .map_err(|_| ProtocolError::InvalidHandshake)??;

    match their_hello {
        Message::Hello { port, user_agent } => {
            tracing::info!(peer = %peer, %port, %user_agent, "Outbound handshake OK");
            Ok(())
        }
        _ => Err(ProtocolError::InvalidHandshake),
    }
}

pub async fn run_message_loop(
    peer: Peer,
    peers_map: Arc<DashMap<Peer, ()>>,
) -> Result<(), ProtocolError> {
    let stream = TcpStream::connect((peer.host.as_str(), peer.port)).await?;
    let (r, w) = split(stream);
    let mut reader = BufReader::new(r);
    let mut writer = BufWriter::new(w);

    let gp = Message::mk_getpeers();
    write_frame(&mut writer, &gp).await?;

    loop {
        let msg = match read_frame(&mut reader).await {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(peer = %peer, error = %e, "Read error in outbound loop");
                return Ok(());
            }
        };

        match msg {
            Message::GetPeers => {
                let list: Vec<Peer> = peers_map.iter().map(|e| e.key().clone()).collect();
                let reply = Message::mk_peers(list);
                write_frame(&mut writer, &reply).await?;
            }
            Message::Peers { peers } => {
                for newp in peers {
                    peers_map.insert(newp, ());
                }
            }
            Message::Error { name, msg } => {
                tracing::debug!(peer = %peer, %name, %msg, "Peer error");
            }
            other => {
                tracing::debug!(peer = %peer, ?other, "Ignored message");
            }
        }
    }
}
