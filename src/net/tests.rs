use tempfile::NamedTempFile;

use crate::{
    config::Config,
    net::{
        framing::{read_frame, write_frame},
        listener::serve,
    },
    protocol::{message::Message, peerlist::Peer},
    state::peers,
};
use core::panic;
use dashmap::DashMap;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader, BufWriter, ReadHalf, WriteHalf, split},
    net::{TcpListener, TcpStream},
    time::{sleep, timeout},
};

async fn util_spawn_test_server()
-> (NamedTempFile, u16, Arc<DashMap<Peer, ()>>, tokio::task::JoinHandle<()>) {
    let tmpfile = NamedTempFile::new().unwrap(); // creating file
    let peers_path: PathBuf = tmpfile.path().to_path_buf();

    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let peers_map = peers::load_from_disk(&peers_path);
    let cfg =
        Arc::new(Config { port, user_agent: "test/0.1".into(), peers_file: peers_path.clone(), max_outbound_connection: 4 });

    let server_task = {
        let peers_map = peers_map.clone();
        let cfg: Arc<Config> = cfg.clone();
        tokio::spawn(async move {
            serve(listener, cfg, peers_map).await.unwrap();
        })
    };

    // give the listener a moment to bind
    sleep(Duration::from_millis(50)).await;
    (tmpfile, port, peers_map, server_task)
}

async fn util_connect_to(
    port: u16,
) -> (BufReader<ReadHalf<TcpStream>>, BufWriter<WriteHalf<TcpStream>>) {
    let sock = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let (r, w) = split(sock);
    (BufReader::new(r), BufWriter::new(w))
}

async fn util_complete_handshake(
    reader: &mut BufReader<ReadHalf<TcpStream>>,
    writer: &mut BufWriter<WriteHalf<TcpStream>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let client_hello = Message::mk_hello(11111, "test_client".into());
    write_frame(writer, &client_hello).await.unwrap();
    let _server_hello = read_frame(reader).await?;

    let _initial_gp = read_frame(reader).await?;

    Ok(())
}

#[tokio::test]
async fn accept_hello_and_replies() {
    let (_peer_file, port, _peers_map, server) = util_spawn_test_server().await;
    let (mut reader, mut writer) = util_connect_to(port).await;

    let client_hello = Message::mk_hello(12345, "foo_agent".into());
    write_frame(&mut writer, &client_hello).await.unwrap();

    sleep(Duration::from_secs(1)).await;

    let resp1 = timeout(Duration::from_secs(1), read_frame(&mut reader)).await.unwrap().unwrap(); //TODO: double unwrap seems weird
    assert!(matches!(resp1, Message::Hello { .. }));

    let resp2 = timeout(Duration::from_secs(1), read_frame(&mut reader)).await.unwrap().unwrap();
    assert!(matches!(resp2, Message::GetPeers));

    server.abort();
}

#[tokio::test]
async fn test_getpeers_empty() {
    let (_peer_file, port, _peers_map, server) = util_spawn_test_server().await;
    let (mut reader, mut writer) = util_connect_to(port).await;
    util_complete_handshake(&mut reader, &mut writer).await.unwrap();

    assert_eq!(_peers_map.len(), 0);

    // we are asking for peers
    let other_getpeers = Message::mk_getpeers();
    write_frame(&mut writer, &other_getpeers).await.unwrap();

    let resp = read_frame(&mut reader).await;
    match resp {
        Ok(Message::Peers { peers }) => {
            assert_eq!(peers.len(), 0);
        }
        other => panic!("expected Peers{{[]}}, got {:?}", other),
    }

    server.abort();
}

#[tokio::test]
async fn test_peers_message_appends() {
    let (tmp_peer_file, port, _, server) = util_spawn_test_server().await;

    // we prepopulate peer A in the peers file
    let peer_a = Peer::try_from("127.0.0.1:1111").unwrap();
    std::fs::write(tmp_peer_file.path(), format!("{},{}\n", peer_a.host, peer_a.port)).unwrap();
    let _ = peers::load_from_disk(tmp_peer_file.path()); // reload internal memory

    // connect and handshake
    let (mut reader, mut writer) = util_connect_to(port).await;
    util_complete_handshake(&mut reader, &mut writer).await.unwrap();

    // send Peers B and C
    let peer_b = Peer::try_from("127.0.0.2:2222").unwrap();
    let peer_c = Peer::try_from("127.0.0.3:3333").unwrap();
    let peers_msg = Message::mk_peers(vec![peer_b.clone(), peer_c.clone()]);
    write_frame(&mut writer, &peers_msg).await.unwrap();

    sleep(Duration::from_millis(50)).await;

    // reload from disk and assert A, B, C are present
    let fresh_map = peers::load_from_disk(tmp_peer_file.path());

    assert!(fresh_map.contains_key(&peer_a), "A missing");
    assert!(fresh_map.contains_key(&peer_b), "B missing");
    assert!(fresh_map.contains_key(&peer_c), "C missing");

    server.abort();
}

#[tokio::test]
async fn rejects_wrong_first_message() {
    let (_peer_file, port, _peers_map, server) = util_spawn_test_server().await;
    let (mut reader, mut writer) = util_connect_to(port).await;

    let client_gp = Message::mk_getpeers();
    write_frame(&mut writer, &client_gp).await.unwrap();

    let err_msg = timeout(Duration::from_secs(1), read_frame(&mut reader)).await.unwrap().unwrap();
    match err_msg {
        Message::Error { name, .. } if name == "INVALID_HANDSHAKE" => {}
        other => panic!("expected INVALID_HANDSHAKE, got {:?}", other),
    }

    let eof = reader.read_line(&mut String::new()).await.unwrap();
    assert_eq!(eof, 0, "socket not closed after invalid handshake");

    server.abort();
}
