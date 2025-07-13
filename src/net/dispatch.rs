use std::sync::Arc;

use dashmap::DashMap;
use rand::{rng, seq::SliceRandom};
use tokio::io::{AsyncBufRead, AsyncWrite};

use crate::{
    config::Config,
    error::ProtocolError,
    net::framing::{read_frame, write_frame},
    protocol::{message::Message, peerlist::Peer},
    state::peers::{PeersError, append_peer},
};

const MAX_PEERS_PER_REPLY: usize = 10;

/// Pulls frames forever, applies the same dispatch logic for both inbound & outbound.
/// Returns when the peer disconnects or a non-recoverable error occurs.
pub async fn run_message_loop<R, W>(
    mut reader: R,
    mut writer: W,
    config: Arc<Config>,
    peers_map: Arc<DashMap<Peer, ()>>,
) -> Result<(), ProtocolError>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    loop {
        let msg = match read_frame(&mut reader).await {
            Ok(m) => m,
            Err(ProtocolError::InvalidFormat | ProtocolError::OversizedFrame) => {
                let err_msg: Message = ProtocolError::InvalidFormat.into();
                write_frame(&mut writer, &err_msg).await?;
                return Ok(());
            }
            Err(e) => return Err(e),
        };

        match msg {
            Message::GetPeers => {
                tracing::debug!(%config.user_agent, "Received GetPeers message");

                let our_peers_to_send = pick_and_build_peers(&config, &peers_map).await;
                let msg = Message::mk_peers(our_peers_to_send);
                write_frame(&mut writer, &msg).await?;
            }
            Message::Peers { peers } => {
                if let Err(e) = import_peers(&config, &peers_map, peers) {
                    tracing::warn!(error = %e, "Failed to store incoming peers");
                }
            }
            Message::Error { name, msg } => {
                tracing::debug!(%name, %msg, "Peer error");
            }
            Message::Hello { port, user_agent } => {
                tracing::debug!(%port, %user_agent, "Received unexpected Hello message, ignoring")
            }
            Message::Unknown => {
                tracing::debug!("Received unexpected message type, ignoring");
            }
        }
    }
}

async fn pick_and_build_peers(config: &Config, peers_map: &DashMap<Peer, ()>) -> Vec<Peer> {
    let mut rng = rng();
    let mut candidates: Vec<Peer> = peers_map
        .iter()
        .map(|e| e.key().clone())
        .filter(|p| !config.banned_hosts.contains(p))
        .collect();

    candidates.shuffle(&mut rng);
    candidates.truncate(MAX_PEERS_PER_REPLY); // TODO: stop hardcoding that, and keep track of inbound connection to set a max

    if config.public_node {
        let me = Peer { host: config.my_host.clone(), port: config.port };
        candidates.retain(|p| p != &me);
        candidates.insert(0, me);
    }
    candidates
}

fn import_peers(
    config: &Config,
    peers_map: &DashMap<Peer, ()>,
    incoming: Vec<Peer>,
) -> Result<(), PeersError> {
    let candidates: Vec<Peer> =
        incoming.iter().filter(|p| !&config.banned_hosts.contains(p)).cloned().collect();
    tracing::debug!(?candidates, "I am {}, Adding their peers to mine", &config.user_agent);
    for peer in candidates {
        append_peer(&config.peers_file, &peers_map, &peer)?;
    }
    Ok(())
}
