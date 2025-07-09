// src/net/tests.rs
use crate::{
    config::Config,
    net::{
        framing::{read_frame, write_frame},
        listener::serve,
    },
    protocol::{message::Message, peerlist::Peer},
};
use core::panic;
use dashmap::DashMap;
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader, BufWriter, split},
    net::{TcpListener, TcpStream},
    time::{sleep, timeout},
};

#[tokio::test]
async fn accept_hello_and_replies() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let peers_map: Arc<DashMap<Peer, ()>> = Arc::new(DashMap::new());

    let cfg = Arc::new(Config { port, user_agent: "foo_user".into() });
    let server_task = tokio::spawn({
        let cfg = cfg.clone();
        async move {
            serve(listener, cfg, peers_map).await.unwrap();
        }
    });

    sleep(Duration::from_millis(50)).await;

    let sock_client: TcpStream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let (reader, writer) = split(sock_client);
    let mut reader = BufReader::new(reader);
    let mut writer = BufWriter::new(writer);

    let client_hello = Message::mk_hello(12345, "foo_agent".into());
    write_frame(&mut writer, &client_hello).await.unwrap();

    sleep(Duration::from_secs(1)).await;

    let resp1 = timeout(Duration::from_secs(1), read_frame(&mut reader)).await.unwrap().unwrap(); //TODO: double unwrap seems weird
    assert!(matches!(resp1, Message::Hello { .. }));

    let resp2 = timeout(Duration::from_secs(1), read_frame(&mut reader)).await.unwrap().unwrap();
    assert!(matches!(resp2, Message::GetPeers));

    server_task.abort();
}

#[tokio::test]
async fn rejects_wrong_first_message() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let peers_map: Arc<DashMap<Peer, ()>> = Arc::new(DashMap::new());

    let cfg = Arc::new(Config::default());
    let server_task = tokio::spawn({
        let cfg = cfg.clone();
        async move {
            serve(listener, cfg, peers_map).await.unwrap();
        }
    });

    sleep(Duration::from_millis(50)).await;

    let sock_client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let (reader, writer) = split(sock_client);
    let mut reader = BufReader::new(reader);
    let mut writer = BufWriter::new(writer);

    let client_gp = Message::mk_getpeers();
    write_frame(&mut writer, &client_gp).await.unwrap();

    let err_msg = timeout(Duration::from_secs(1), read_frame(&mut reader)).await.unwrap().unwrap();
    match err_msg {
        Message::Error { name, .. } if name == "INVALID_HANDSHAKE" => {}
        other => panic!("expected INVALID_HANDSHAKE, got {:?}", other),
    }

    let eof = reader.read_line(&mut String::new()).await.unwrap();
    assert_eq!(eof, 0, "socket not closed after invalid handshake");

    server_task.abort();
}
