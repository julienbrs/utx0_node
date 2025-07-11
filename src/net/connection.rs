use std::time::Duration;

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

pub async fn connect_and_handshake(
    config: &Config,
    peer: &Peer,
) -> Result<
    (BufReader<tokio::io::ReadHalf<TcpStream>>, BufWriter<tokio::io::WriteHalf<TcpStream>>),
    ProtocolError,
> {
    tracing::info!(peer = %peer, "Dialing outbound peer");
    let stream = TcpStream::connect((peer.host.as_str(), peer.port)).await?;
    let (r, w) = split(stream);
    let mut reader: BufReader<tokio::io::ReadHalf<TcpStream>> = BufReader::new(r);
    let mut writer: BufWriter<tokio::io::WriteHalf<TcpStream>> = BufWriter::new(w);

    let hello = Message::mk_hello(config.port, config.user_agent.clone());
    write_frame(&mut writer, &hello).await?;

    let their_hello = tokio::time::timeout(
        Duration::from_secs(config.service_loop_delay),
        read_frame(&mut reader),
    )
    .await
    .map_err(|_| ProtocolError::InvalidHandshake)??;

    match their_hello {
        Message::Hello { port, user_agent } => {
            tracing::info!(peer = %peer, %port, %user_agent, "Outbound handshake OK");
            Ok((reader, writer))
        }
        _ => Err(ProtocolError::InvalidHandshake),
    }
}

#[cfg(test)]
mod tests {

    use std::time::Duration;

    use tokio::{
        io::{BufReader, BufWriter},
        net::TcpListener,
    };

    use crate::{
        config::Config,
        error::ProtocolError,
        net::{
            connection::connect_and_handshake,
            framing::{read_frame, write_frame},
        },
        protocol::{message::Message, peerlist::Peer},
    };

    #[tokio::test]
    async fn handshake_ok() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            let (r, w) = tokio::io::split(sock);
            let mut reader = BufReader::new(r);
            let mut writer = BufWriter::new(w);

            // read the client's hello (sent in connect and handshake)
            let msg = read_frame(&mut reader).await.unwrap();
            assert!(matches!(msg, Message::Hello { .. }));
            // send back hello to the client for the read_frame call in connect_and_handshake
            let hello = Message::mk_hello(port, "srv".into());
            write_frame(&mut writer, &hello).await.unwrap();
        });

        let cfg = Config::default();
        let peer = Peer { host: "127.0.0.1".into(), port };
        let res = connect_and_handshake(&cfg, &peer).await;
        assert!(res.is_ok());
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
        let res = connect_and_handshake(&cfg, &peer).await;
        dbg!(&res);
        assert!(matches!(res, Err(ProtocolError::InvalidHandshake)));
    }
}
