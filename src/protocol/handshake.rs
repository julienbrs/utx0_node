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

#[cfg(test)]
mod tests {

    use std::time::Duration;

    use tokio::{
        io::{BufReader, BufWriter, split},
        net::{TcpListener, TcpStream},
        time::timeout,
    };

    use crate::{
        config::Config,
        error::ProtocolError,
        net::framing::{read_frame, write_frame},
        protocol::{
            handshake::{inbound, outbound},
            message::Message,
            peerlist::Peer,
        },
    };

    #[tokio::test]
    async fn outbound_ok() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();

        // make a “server” that accepts, does exactly the inbound handshake,
        // then waits for the peer’s GetPeers:
        let server = tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            let (mut reader, mut _writer) =
                inbound(sock, &Config::new_test(port, "srv", "/tmp/peers.csv")).await.unwrap();

            // server inbound() actually already sent Hello+GetPeers,
            // now read the dialer’s GetPeers:
            let msg = read_frame(&mut reader).await.unwrap();
            assert!(matches!(msg, Message::GetPeers));
        });

        // client outbound handshake:
        let cfg = Config::new_test(0, "cli", "/tmp/peers.csv");
        let peer = Peer { host: "127.0.0.1".into(), port };
        let (mut reader, mut _writer) = outbound(&cfg, &peer).await.unwrap();

        // outbound() already sent GetPeers on its own, so server sees it.
        // now client should read the server’s GetPeers:
        let msg = read_frame(&mut reader).await.unwrap();
        assert!(matches!(msg, Message::GetPeers));

        server.abort();
    }

    #[tokio::test]
    async fn handshake_timeout() {
        // server never sends back hello
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let _server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            // do nothing to timeout in connect_and_handshake
            tokio::time::sleep(Duration::from_secs(60)).await;
        });

        let mut cfg = Config::new(0, "test", "peers.csv");
        cfg.max_outbound_connection = 4;
        cfg.service_loop_delay = 1;

        let peer = Peer { host: "127.0.0.1".into(), port };
        let res = outbound(&cfg, &peer).await;
        dbg!(&res);
        assert!(matches!(res, Err(ProtocolError::InvalidHandshake)));
    }

    #[tokio::test]
    async fn inbound_ok() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let cfg = Config::new_test(port, "srv", "/tmp/peers.csv");

        // spawn the server handshake
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let (_reader, _writer) = inbound(socket, &cfg).await.unwrap();
            // inbound() itself already sent Hello + GetPeers
        });

        // client side: dial + send Hello
        let sock = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let (r, w) = split(sock);
        let mut reader = BufReader::new(r);
        let mut writer = BufWriter::new(w);

        let client_hello = Message::mk_hello(12345, "cli".into());
        write_frame(&mut writer, &client_hello).await.unwrap();

        // now client should see server Hello then GetPeers
        let resp1 =
            timeout(Duration::from_secs(1), read_frame(&mut reader)).await.unwrap().unwrap();
        assert!(matches!(resp1, Message::Hello { .. }));

        let resp2 =
            timeout(Duration::from_secs(1), read_frame(&mut reader)).await.unwrap().unwrap();
        assert!(matches!(resp2, Message::GetPeers));

        server.abort();
    }

    #[tokio::test]
    async fn inbound_invalid_first_message() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let cfg = Config::new_test(port, "srv", "/tmp/peers.csv");

        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let res = inbound(socket, &cfg).await;
            assert!(matches!(res, Err(ProtocolError::InvalidHandshake)));
        });

        // client side: dial + send *wrong* first message
        let sock = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let (r, w) = split(sock);
        let mut reader = BufReader::new(r);
        let mut writer = BufWriter::new(w);

        let client_bad = Message::mk_getpeers();
        write_frame(&mut writer, &client_bad).await.unwrap();

        // server should immediately close after sending the Error
        let err = timeout(Duration::from_secs(1), read_frame(&mut reader)).await.unwrap().unwrap();
        assert!(matches!(err, Message::Error { name, .. } if name == "INVALID_HANDSHAKE"));

        server.abort();
    }
}
