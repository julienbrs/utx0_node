use tempfile::NamedTempFile;

use crate::{
    config::Config,
    net::{
        client::outbound_loop,
        framing::{read_frame, write_frame},
        housekeeping::housekeeping_loop,
        listener::start_listening,
    },
    protocol::{message::Message, peerlist::Peer},
    state::{
        connection::{InboundCounter, new_outbound_map},
        peers,
    },
    storage::RedbStore,
    util::constants::RECV_BUFFER_LIMIT,
};
use core::panic;
use dashmap::DashMap;
use std::{
    path::PathBuf,
    sync::{Arc, Once},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter, ReadHalf, WriteHalf, split},
    net::{TcpListener, TcpStream},
    sync::Barrier,
    task::JoinHandle,
    time::sleep,
};

static INIT: Once = Once::new();

fn init_tracing() {
    INIT.call_once(|| {
        tracing_subscriber::fmt().with_max_level(tracing::Level::DEBUG).with_test_writer().init();
    });
}

async fn util_spawn_test_server()
-> (NamedTempFile, u16, Arc<RedbStore>, Arc<DashMap<Peer, ()>>, tokio::task::JoinHandle<()>) {
    let tmpfile = NamedTempFile::new().unwrap(); // creating file
    let peers_path: PathBuf = tmpfile.path().to_path_buf();

    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let peers_map = peers::load_from_disk(&peers_path);
    let cfg = Arc::new(Config::new_test(port, "test/0.1", tmpfile.path()));
    let db_file = NamedTempFile::new().unwrap();
    let store = RedbStore::new(db_file.path()).expect("redb store creation");
    let store = Arc::new(store);

    let ic = InboundCounter::new();

    let server_task = {
        let peers_map = peers_map.clone();
        let cfg = cfg.clone();
        let store = store.clone();
        tokio::spawn(async move {
            start_listening(cfg, store, peers_map, ic).await.unwrap();
        })
    };

    // give the listener a moment to bind
    sleep(Duration::from_millis(50)).await;
    (tmpfile, port, store, peers_map, server_task)
}

pub async fn util_connect_to(
    port: u16,
) -> (BufReader<ReadHalf<TcpStream>>, BufWriter<WriteHalf<TcpStream>>) {
    let sock = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let (r, w) = split(sock);
    (BufReader::new(r), BufWriter::new(w))
}

async fn util_complete_handshake(
    config: Arc<Config>,
    reader: &mut BufReader<ReadHalf<TcpStream>>,
    writer: &mut BufWriter<WriteHalf<TcpStream>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let client_hello = Message::mk_hello(config.port, config.user_agent.clone());
    write_frame(writer, &client_hello).await.unwrap();
    let _server_hello = read_frame(reader).await?;

    let _initial_gp = read_frame(reader).await?;
    tracing::debug!(%config.port, %config.user_agent, "Handshake completed");

    Ok(())
}

#[tokio::test]
async fn test_getpeers_empty() {
    let (_peer_file, port, _store, _peers_map, server) = util_spawn_test_server().await;
    let (mut reader, mut writer) = util_connect_to(port).await;
    let config = Arc::new(Config::new_test(port, "node1", _peer_file.path()));
    util_complete_handshake(config, &mut reader, &mut writer).await.unwrap();

    assert_eq!(_peers_map.len(), 0);

    // since there are no *other* peers, we expect exactly our own address back
    let getpeers = Message::mk_getpeers();
    write_frame(&mut writer, &getpeers).await.unwrap();

    let resp = read_frame(&mut reader).await.expect("did not get a Peers reply");
    if let Message::Peers { peers } = resp {
        // exactly one: ourselves
        assert_eq!(peers.len(), 1);
        let me = Peer { host: "0.0.0.0".into(), port };
        assert_eq!(peers[0], me);
    } else {
        panic!("expected Peers{{…}}, got {:?}", resp);
    }

    server.abort();
}

#[tokio::test]
async fn test_peers_message_appends() {
    let (tmp_peer_file, port, _store, _, server) = util_spawn_test_server().await;
    let config = Arc::new(Config::new_test(port, "node1", tmp_peer_file.path()));

    // we prepopulate peer A in the peers file
    let peer_a = Peer::try_from("127.0.0.1:1111").unwrap();
    std::fs::write(tmp_peer_file.path(), format!("{},{}\n", peer_a.host, peer_a.port)).unwrap();
    let _ = peers::load_from_disk(tmp_peer_file.path()); // reload internal memory

    // connect and handshake
    let (mut reader, mut writer) = util_connect_to(port).await;
    util_complete_handshake(config, &mut reader, &mut writer).await.unwrap();

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
async fn malformed_json_extra_field_rejected() {
    init_tracing();

    let (_peer_file, port, _store, _peers_map, server) = util_spawn_test_server().await;
    let config = Arc::new(Config::new_test(port, "node1", _peer_file.path()));

    let (mut reader, mut writer) = util_connect_to(port).await;
    util_complete_handshake(config, &mut reader, &mut writer).await.unwrap();

    // sending GetPeers with an unknown extra key
    writer.write_all(b"{\"type\":\"GetPeers\",\"unexpected\":123}\n").await.unwrap();
    writer.flush().await.unwrap();

    // expecting one Error(INVALID_FORMAT) then socket close
    let err = read_frame(&mut reader).await.unwrap();
    if let Message::Error { name, .. } = err {
        assert_eq!(name, "INVALID_FORMAT");
    } else {
        panic!("expected INVALID_FORMAT error, got {:?}", err);
    }

    // socket must now be closed
    let eof = reader.read_line(&mut String::new()).await.unwrap();
    assert_eq!(eof, 0);

    server.abort();
}

#[tokio::test]
async fn unknown_message_type_ignored() {
    // init_tracing();
    let (_peer_file, port, _store, _peers_map, server) = util_spawn_test_server().await;
    let (mut reader, mut writer) = util_connect_to(port).await;
    let config = Arc::new(Config::new_test(port, "node1", _peer_file.path()));

    util_complete_handshake(config, &mut reader, &mut writer).await.unwrap();

    // send a totally unknown type
    writer.write_all(b"{\"type\":\"Foobar\"}\n").await.unwrap();
    writer.flush().await.unwrap();

    // no reply, connection should stay open.  Now send a valid GetPeers
    writer.write_all(b"{\"type\":\"GetPeers\"}\n").await.unwrap();
    writer.flush().await.unwrap();

    // we should still get back a Peers response
    let resp = read_frame(&mut reader).await.expect("expected Peers reply");
    assert!(matches!(resp, Message::Peers { .. }));

    server.abort();
}

#[tokio::test]
async fn malformed_json_missing_field_rejected() {
    init_tracing();
    let (_peer_file, port, _store, _peers_map, server) = util_spawn_test_server().await;
    let (mut reader, mut writer) = util_connect_to(port).await;
    let config = Arc::new(Config::new_test(port, "node1", _peer_file.path()));

    util_complete_handshake(config, &mut reader, &mut writer).await.unwrap();

    // message without field
    writer.write_all(b"{\"type\":\"Foobar\"}\n").await.unwrap();
    writer.flush().await.unwrap();

    // we should still get back a Peers response
    writer.write_all(b"{\"type\":\"GetPeers\"}\n").await.unwrap();
    writer.flush().await.unwrap();

    // we should still get back a Peers response
    let resp = read_frame(&mut reader).await.expect("Expected Peers reply");

    assert!(matches!(resp, Message::Peers { .. }));

    server.abort();
}

#[tokio::test]
async fn malformed_json_truncated_rejected() {
    init_tracing();
    let (_peer_file, port, _store, _peers_map, server) = util_spawn_test_server().await;
    let (mut reader, mut writer) = util_connect_to(port).await;
    let config = Arc::new(Config::new_test(port, "node1", _peer_file.path()));

    util_complete_handshake(config, &mut reader, &mut writer).await.unwrap();

    // message without field
    writer.write_all(b"{\"type\":\"GetPeers\"\n").await.unwrap();
    writer.flush().await.unwrap();

    let resp = read_frame(&mut reader).await.expect("Expected Error reply");

    tracing::info!(?resp, "resp");
    assert!(matches!(resp, Message::Error { .. }));

    if let Message::Error { name, .. } = resp {
        assert_eq!(name, "INVALID_FORMAT");
    } else {
        panic!("expected INVALID_FORMAT error, got {:?}", resp);
    }

    // socket must now be closed
    let eof = reader.read_line(&mut String::new()).await.unwrap();
    assert_eq!(eof, 0);

    server.abort();
}

#[tokio::test]
async fn oversized_json_rejected() {
    init_tracing();
    let (_peer_file, port, _store, _peers_map, server) = util_spawn_test_server().await;
    let (mut reader, mut writer) = util_connect_to(port).await;
    let config = Arc::new(Config::new_test(port, "node1", _peer_file.path()));

    util_complete_handshake(config, &mut reader, &mut writer).await.unwrap();

    // oversized JSON
    let repeated = "x".repeat(RECV_BUFFER_LIMIT);
    writer.write_all(repeated.as_bytes()).await.unwrap();
    writer.flush().await.unwrap();

    let resp = read_frame(&mut reader).await.expect("Expected Error reply");

    tracing::info!(?resp, "resp");
    assert!(matches!(resp, Message::Error { .. }));

    if let Message::Error { name, .. } = resp {
        assert_eq!(name, "OVERSIZED_FRAME");
    } else {
        panic!("expected OVERSIZED_FRAME error, got {:?}", resp);
    }

    // socket must now be closed
    let eof = reader.read_line(&mut String::new()).await.unwrap();
    assert_eq!(eof, 0);

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
    let store_foo = RedbStore::new("objects1.db")
        .unwrap_or_else(|e| panic!("Error during creation of redb db: {e}"));
    let store_foo = Arc::new(store_foo);

    let cfg1 = Arc::new(Config::new_test(port1, "node1", file1.path()));
    let cfg2 = Arc::new(Config::new_test(port2, "node2", file2.path()));

    // initialising peers file with address of each other
    let pm1: Arc<DashMap<Peer, ()>> = peers::load_from_disk(&cfg1.peers_file);
    pm1.insert(Peer { host: "127.0.0.1".into(), port: port2 }, ());

    let pm2: Arc<DashMap<Peer, ()>> = peers::load_from_disk(&cfg2.peers_file);
    pm2.insert(Peer { host: "127.0.0.1".into(), port: port1 }, ());

    // outbound and inbound connections empty at start
    let oc1 = new_outbound_map();
    let oc2 = new_outbound_map();
    let ic1 = InboundCounter::new();
    let ic2 = InboundCounter::new();

    let barrier = Arc::new(Barrier::new(4));

    // starting the 2 listeners
    let b1 = barrier.clone();
    let l1_task = {
        let cfg = cfg1.clone();
        let pm = pm1.clone();
        let store_foo = store_foo.clone();
        tokio::spawn(async move {
            b1.wait().await;
            start_listening(cfg, store_foo, pm, ic1).await.unwrap();
        })
    };
    let b2 = barrier.clone();
    let l2_task = {
        let cfg = cfg2.clone();
        let pm = pm2.clone();
        let store_foo = store_foo.clone();
        tokio::spawn(async move {
            b2.wait().await;
            start_listening(cfg, store_foo, pm, ic2).await.unwrap();
        })
    };

    // starting the 2 outbound loops
    let b3 = barrier.clone();
    let o1_task = {
        let cfg = cfg1.clone();
        let pm = pm1.clone();
        let out = oc1.clone();
        let store_foo = store_foo.clone();
        tokio::spawn(async move {
            b3.wait().await;
            outbound_loop(cfg, store_foo, out, pm).await;
        })
    };
    let b4 = barrier.clone();
    let o2_task = {
        let cfg = cfg2.clone();
        let pm = pm2.clone();
        let out = oc2.clone();
        let store_foo = store_foo.clone();
        tokio::spawn(async move {
            b4.wait().await;
            outbound_loop(cfg, store_foo, out, pm).await;
        })
    };

    sleep(Duration::from_secs(cfg1.service_loop_delay * 2)).await;

    let peer1 = Peer { host: "127.0.0.1".into(), port: port2 };
    let peer2 = Peer { host: "127.0.0.1".into(), port: port1 };

    assert!(oc1.contains_key(&peer1), "Node1 did not open connection to Node2");
    assert!(oc2.contains_key(&peer2), "Node2 did not open connection to Node1");

    l1_task.abort();
    l2_task.abort();
    o1_task.abort();
    o2_task.abort();
}

#[tokio::test]
async fn discovery_via_intermediary() {
    // init_tracing();
    let l1 = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port1 = l1.local_addr().unwrap().port();
    let l2 = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port2 = l2.local_addr().unwrap().port();
    let l3 = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port3 = l3.local_addr().unwrap().port();
    drop(l1);
    drop(l2);
    drop(l3);
    let peer1 = Peer { host: "127.0.0.1".into(), port: port1 };
    tracing::debug!(%peer1, "Aiming to propagate peer1");

    let file1 = NamedTempFile::new().unwrap();
    let file2 = NamedTempFile::new().unwrap();
    let file3 = NamedTempFile::new().unwrap();

    let db1 = NamedTempFile::new().unwrap();
    let store1 = RedbStore::new(db1.path()).unwrap();
    let store1 = Arc::new(store1);

    let db2 = NamedTempFile::new().unwrap();
    let store2 = RedbStore::new(db2.path()).unwrap();
    let store2 = Arc::new(store2);

    let db3 = NamedTempFile::new().unwrap();
    let store3 = RedbStore::new(db3.path()).unwrap();
    let store3 = Arc::new(store3);

    let cfg1 = Arc::new(Config::new_test(port1, "node1", file1.path()));
    let cfg2 = Arc::new(Config::new_test(port2, "node2", file2.path()));
    let cfg3 = Arc::new(Config::new_test(port3, "node3", file3.path()));

    let pm1: Arc<DashMap<Peer, ()>> = peers::load_from_disk(&cfg1.peers_file);
    let pm2: Arc<DashMap<Peer, ()>> = peers::load_from_disk(&cfg2.peers_file);
    let pm3: Arc<DashMap<Peer, ()>> = peers::load_from_disk(&cfg3.peers_file);

    pm2.insert(Peer { host: "127.0.0.1".into(), port: port1 }, ());
    pm3.insert(Peer { host: "127.0.0.1".into(), port: port2 }, ());

    let oc2 = new_outbound_map();
    let oc3 = new_outbound_map();
    let ic2 = InboundCounter::new();
    let ic3 = InboundCounter::new();

    let barrier = Arc::new(Barrier::new(5));

    let b1 = barrier.clone();
    let l1_task = {
        let cfg = cfg1.clone();
        let pm = pm1.clone();
        let store1 = store1.clone();
        tokio::spawn(async move {
            b1.wait().await;
            start_listening(cfg, store1, pm, ic2).await.unwrap();
        })
    };
    let b2 = barrier.clone();
    let l2_task = {
        let cfg = cfg2.clone();
        let pm = pm2.clone();
        let ic3 = ic3.clone();
        let store2 = store2.clone();
        tokio::spawn(async move {
            b2.wait().await;
            start_listening(cfg, store2, pm, ic3).await.unwrap();
        })
    };
    let b3 = barrier.clone();
    let l3_task = {
        let cfg = cfg3.clone();
        let pm = pm3.clone();
        let ic3 = ic3.clone();
        let store3 = store3.clone();
        tokio::spawn(async move {
            b3.wait().await;
            start_listening(cfg, store3, pm, ic3).await.unwrap();
        })
    };

    let b4 = barrier.clone();
    let o2_task = {
        let cfg = cfg2.clone();
        let pm = pm2.clone();
        let oc = oc2.clone();
        let store2 = store2.clone();
        tokio::spawn(async move {
            b4.wait().await;
            outbound_loop(cfg, store2, oc, pm).await;
        })
    };
    let b5 = barrier.clone();
    let o3_task = {
        let cfg = cfg3.clone();
        let pm = pm3.clone();
        let oc = oc3.clone();
        let store3 = store3.clone();
        tokio::spawn(async move {
            b5.wait().await;
            outbound_loop(cfg, store3, oc, pm).await;
        })
    };

    sleep(Duration::from_secs(3)).await;

    assert!(oc3.contains_key(&peer1), "Node3 did not discover Node1 via Node2");

    l1_task.abort();
    l2_task.abort();
    l3_task.abort();
    o2_task.abort();
    o3_task.abort();
}

#[tokio::test]
async fn test_housekeeping_prunes_finished_outbound() {
    init_tracing();
    let cfg = Arc::new(Config::new_test(0, "node1", "peers.csv"));
    let peers_map = Arc::new(DashMap::new());
    let outbound = new_outbound_map();
    let inbound_ctr = InboundCounter::new();

    tokio::spawn({
        let cfg = cfg.clone();
        let om = outbound.clone();
        let pm = peers_map.clone();
        let ic = inbound_ctr.clone();
        async move {
            // boucle infinie qui retient et prune
            housekeeping_loop(cfg, ic, om, pm).await;
        }
    });

    // 2 tasks, one shutting down instantly, the other one in 60 sec
    let peer1 = Peer { host: "127.0.0.1".into(), port: 1001 };
    let peer2 = Peer { host: "127.0.0.1".into(), port: 1002 };

    let h1: JoinHandle<()> = tokio::spawn(async {
        // task does nothing and cuts
    });
    let h2: JoinHandle<()> = tokio::spawn(async {
        sleep(Duration::from_secs(60)).await;
    });

    // runtime executing h1
    tokio::task::yield_now().await;

    // insert 2 handles in outbound_map
    outbound.insert(peer1.clone(), h1);
    outbound.insert(peer2.clone(), h2);

    // we wait 2 housekeeping cycles
    sleep(Duration::from_secs(cfg.service_loop_delay * 2)).await;

    // h2 should still run
    assert_eq!(outbound.len(), 1);
    assert!(outbound.contains_key(&peer2));
}

#[tokio::test]
async fn inbound_counter_counts_active_connections() {
    let port = TcpListener::bind(("127.0.0.1", 0)).await.unwrap().local_addr().unwrap().port();

    let tmpfile = NamedTempFile::new().unwrap();
    let peers_map = peers::load_from_disk(tmpfile.path());
    let cfg = Arc::new(Config::new_test(port, "node1", tmpfile.path()));
    let store = RedbStore::new("objects.db")
        .unwrap_or_else(|e| panic!("Error during creation of redb db: {e}"));
    let store = Arc::new(store);
    let inbound = InboundCounter::new();

    let server = {
        let cfg = cfg.clone();
        let pm = peers_map.clone();
        let ic = inbound.clone();
        tokio::spawn(async move {
            start_listening(cfg, store, pm, ic).await.unwrap();
        })
    };

    sleep(Duration::from_millis(50)).await;
    assert_eq!(inbound.load(), 0, "no inbound connexion at start");

    // open client connexion + handshake
    let sock = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let (r, mut w) = split(sock);
    let mut rdr = BufReader::new(r);

    let hello = Message::mk_hello(cfg.port, cfg.user_agent.clone());
    write_frame(&mut w, &hello).await.unwrap();
    let _ = read_frame(&mut rdr).await.unwrap(); // server Hello
    let _ = read_frame(&mut rdr).await.unwrap(); // server GetPeers

    assert_eq!(inbound.load(), 1, "inbound connexion should be 1");

    // 5) Fermer la connexion client
    drop(rdr);
    drop(w);
    sleep(Duration::from_millis(100)).await;

    // 6) Le compteur doit retomber à 0
    assert_eq!(inbound.load(), 0, "inbound connexion should be 0 at the end");

    // 7) Abandonner proprement le listener
    server.abort();
}
