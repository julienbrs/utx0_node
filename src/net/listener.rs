use std::sync::Arc;

use crate::{
    config::Config,
    error::ProtocolError,
    net::framing::{read_frame, write_frame},
    protocol::{message::Message, peerlist::Peer},
    state::peers::append_peer,
};
use dashmap::DashMap;
use rand::{rng, seq::SliceRandom};
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

    loop {
        let msg = match read_frame(&mut reader).await {
            Ok(m) => {
                tracing::debug!(?m, "I am {}, Received message", &config.user_agent);
                m
            }
            Err(e) => {
                match e {
                    ProtocolError::OversizedFrame | ProtocolError::InvalidFormat => {
                        tracing::warn!(error = %e, "Malformed frame from peer; closing connection");
                        let err_msg: Message = e.into();
                        write_frame(&mut writer, &err_msg).await?;
                    }

                    ProtocolError::ConnectionClosed => {
                        tracing::info!("Peer closed the connection");
                    }

                    ProtocolError::Io(io_err) => {
                        // sending error back to writer? it might be broken so better not
                        tracing::warn!(error = %io_err, "I/O error reading from socket");
                    }

                    // shouldn't happen
                    ProtocolError::InvalidHandshake => {
                        let err_msg: Message = e.into();
                        let _ = write_frame(&mut writer, &err_msg).await;
                        tracing::warn!("Handshake error in main loop");
                    }
                }
                return Ok(());
            }
        };

        match msg {
            Message::GetPeers => {
                let msg_peers = {
                    let mut rng = rng();
                    let mut candidates: Vec<Peer> = peers_map
                        .iter()
                        .map(|e| e.key().clone())
                        .filter(|p| !config.banned_hosts.contains(p))
                        .collect();

                    candidates.shuffle(&mut rng);
                    candidates.truncate(10); // TODO: stop hardcoding that, and keep track of inbound connection to set a max


                    if config.public_node {
                        let me = Peer {host: config.my_host.clone(), port: config.port};
                        candidates.retain(|p| p != &me);
                        candidates.insert(0, me);
                    }
                    Message::mk_peers(candidates)
                };
                write_frame(&mut writer, &msg_peers).await?;
            }

            Message::Peers { peers } => {
                let candidates: Vec<Peer> = peers.iter().filter(|p| !&config.banned_hosts.contains(p)).cloned().collect();
                tracing::debug!(?candidates, "I am {}, Adding their peers to mine", &config.user_agent);
                for peer in candidates {
                    append_peer(&config.peers_file, &peers_map, &peer).unwrap();
                }
            }

            Message::Error { name, msg } => {
                tracing::debug!(%name, %msg, "Received error from peer; ignoring");
            }

            other => {
                tracing::debug!(?other, "Unhandled message type; ignoring");
            }
        }
    }
}
