use tempfile::NamedTempFile;

use crate::{
    config::Config,
    net::{
        client::outbound_loop,
        framing::{read_frame, write_frame},
        listener::{serve, start_listening},
    },
    protocol::{message::Message, peerlist::Peer},
    state::{connection::new_outbound_map, peers},
};
use core::panic;
use dashmap::DashMap;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader, BufWriter, ReadHalf, WriteHalf, split},
    net::{TcpListener, TcpStream},
    sync::Barrier,
    time::{sleep, timeout},
};

async fn util_spawn_test_server()
-> (NamedTempFile, u16, Arc<DashMap<Peer, ()>>, tokio::task::JoinHandle<()>) {
    let tmpfile = NamedTempFile::new().unwrap(); // creating file
    let peers_path: PathBuf = tmpfile.path().to_path_buf();

    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let peers_map = peers::load_from_disk(&peers_path);
    let cfg = Arc::new(Config {
        port,
        user_agent: "test/0.1".into(),
        peers_file: peers_path.clone(),
        max_outbound_connection: 4,
        service_loop_delay: 2,
    });

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

#[tokio::test]
async fn two_nodes_discover_each_other() {
    let l1 = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port1 = l1.local_addr().unwrap().port();
    let l2 = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port2 = l2.local_addr().unwrap().port();
    // we drop listeners for the moment, we will reopen it with start_listening
    drop(l1);
    drop(l2);

    let file1 = NamedTempFile::new().unwrap();
    let file2 = NamedTempFile::new().unwrap();

    let cfg1 = Config {
        port: port1,
        user_agent: "node1".to_string(),
        peers_file: PathBuf::from(file1.path()),
        max_outbound_connection: 4,
        service_loop_delay: 1,
    };
    let cfg1 = Arc::new(cfg1);
    let cfg2 = Config {
        port: port2,
        user_agent: "node2".to_string(),
        peers_file: PathBuf::from(file2.path()),
        max_outbound_connection: 4,
        service_loop_delay: 1,
    };
    let cfg2 = Arc::new(cfg2);

    // initialising peers file with address of each other
    let pm1: Arc<DashMap<Peer, ()>> = peers::load_from_disk(&cfg1.peers_file);
    pm1.insert(Peer { host: "127.0.0.1".into(), port: port2 }, ());

    let pm2: Arc<DashMap<Peer, ()>> = peers::load_from_disk(&cfg2.peers_file);
    pm2.insert(Peer { host: "127.0.0.1".into(), port: port1 }, ());

    // outbound connections empty at start
    let oc1 = new_outbound_map();
    let oc2 = new_outbound_map();

    let barrier = Arc::new(Barrier::new(4));

    // starting the 2 listeners
    let b1 = barrier.clone();
    let l1_task = {
        let cfg = cfg1.clone();
        let pm = pm1.clone();
        tokio::spawn(async move {
            b1.wait().await;
            start_listening(cfg, pm).await.unwrap();
        })
    };
    let b2 = barrier.clone();
    let l2_task = {
        let cfg = cfg2.clone();
        let pm = pm2.clone();
        tokio::spawn(async move {
            b2.wait().await;
            start_listening(cfg, pm).await.unwrap();
        })
    };

    // starting the 2 outbound loops
    let b3 = barrier.clone();
    let o1_task = {
        let cfg = cfg1.clone();
        let pm = pm1.clone();
        let out = oc1.clone();
        tokio::spawn(async move {
            b3.wait().await;
            outbound_loop(cfg, out, pm).await;
        })
    };
    let b4 = barrier.clone();
    let o2_task = {
        let cfg = cfg2.clone();
        let pm = pm2.clone();
        let out = oc2.clone();
        tokio::spawn(async move {
            b4.wait().await;
            outbound_loop(cfg, out, pm).await;
        })
    };

    sleep(Duration::from_secs(&cfg1.service_loop_delay * 2)).await;

    let peer1 = Peer { host: "127.0.0.1".into(), port: port2 };
    let peer2 = Peer { host: "127.0.0.1".into(), port: port1 };

    assert!(oc1.contains_key(&peer1), "Node1 n'a pas ouvert de connexion vers Node2");
    assert!(oc2.contains_key(&peer2), "Node2 n'a pas ouvert de connexion vers Node1");

    l1_task.abort();
    l2_task.abort();
    o1_task.abort();
    o2_task.abort();
}
