use serde::{Deserialize, Serialize};

use crate::protocol::peerlist::Peer;

// TODO: forbid unwanted extra fields when deserialize w/ deny_unknown_fields. But doesnt work for enum
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(tag = "type")]
pub enum Message {
    Hello { port: u16, user_agent: String },
    Peers { peers: Vec<Peer> },
    GetPeers,
    Error { name: String, msg: String },
}

impl Message {
    pub fn mk_hello(port: u16, user_agent: String) -> Self {
        Self::Hello { port, user_agent }
    }

    pub fn mk_peers(peers: Vec<Peer>) -> Self {
        Self::Peers { peers }
    }

    pub fn mk_getpeers() -> Self {
        Self::GetPeers
    }

    pub fn mk_error(name: String, msg: String) -> Self {
        Self::Error { name, msg }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_back_and_forth_hello() {
        let port = 8333;
        let user_agent = "utx0/rust-v0.0".to_string();
        let hello: Message = Message::Hello { port, user_agent };

        let hello_json = serde_json::to_string(&hello).expect("failed to serialize msg to JSON");
        let decoded_hello: Message =
            serde_json::from_str(&hello_json).expect("failed to deserialize JSON to message");

        assert_eq!(hello, decoded_hello);
    }

    #[test]
    fn test_back_and_forth_peers() {
        let peer1 = Peer { host: "127.0.0.1".to_string(), port: 18018 };
        let peer2 = Peer { host: "node.peer.com".to_string(), port: 18018 };
        let peers: Vec<Peer> = vec![peer1, peer2];
        let peers_msg: Message = Message::Peers { peers };

        let peers_json =
            serde_json::to_string(&peers_msg).expect("failed to serialize msg to JSON");
        let decoded_peers: Message =
            serde_json::from_str(&peers_json).expect("failed to deserialize JSON to message");

        assert_eq!(peers_msg, decoded_peers);
    }

    #[test]
    fn test_back_and_forth_getpeers() {
        let getpeers_msg: Message = Message::GetPeers;

        let getpeers_json =
            serde_json::to_string(&getpeers_msg).expect("failed to serialize msg to JSON");
        let decoded_getpeers: Message =
            serde_json::from_str(&getpeers_json).expect("failed to deserialize JSON to message");

        assert_eq!(getpeers_msg, decoded_getpeers);
    }

    #[test]
    fn test_back_and_forth_error() {
        let error_msg: Message = Message::Error {
            name: "INVALID_FORMAT".to_string(),
            msg: "The format is invalid".to_string(),
        };

        let error_json =
            serde_json::to_string(&error_msg).expect("failed to serialize msg to JSON");
        let decoded_getpeers: Message =
            serde_json::from_str(&error_json).expect("failed to deserialize JSON to message");

        assert_eq!(error_msg, decoded_getpeers);
    }

    #[test]
    fn test_tag_type() {
        let messages = vec![
            (Message::mk_hello(18018, "test".into()), "Hello"),
            (Message::mk_peers(vec![]), "Peers"),
            (Message::mk_getpeers(), "GetPeers"),
            (Message::mk_error("Error_name".into(), "error content".into()), "Error"),
        ];

        for (message, expected_tag) in messages {
            let json = serde_json::to_string(&message).unwrap();
            assert!(
                json.contains(&format!(r#"type":"{expected_tag}"#)),
                "Missing {expected_tag} pattern in {json}"
            );
        }
    }

    #[test]
    fn test_invalid_json_fails() {
        let dummy = r#"{"type":"Hello","port":not_a_number}"#;
        let res: Result<Message, _> = serde_json::from_str(dummy);
        assert!(res.is_err(), "dummy JSON must not deserialize");
    }

    #[test]

    fn test_missing_field_rejected() {
        let missing_port = r#"{"type":"Hello","user_agent":"utx0"}"#;
        let res: Result<Message, _> = serde_json::from_str(missing_port);
        assert!(res.is_err(), "Hello without `port` must be an error");
    }

    #[test]
    fn test_mk_hello() {
        let port = 12345;
        let agent = "utx0-agent".to_string();
        let msg = Message::mk_hello(port, agent.clone());

        assert_eq!(msg, Message::Hello { port, user_agent: agent });
    }

    #[test]
    fn test_mk_peers() {
        let peers = vec![
            Peer { host: "localhost".to_string(), port: 18018 },
            Peer { host: "8.8.8.8".to_string(), port: 18018 },
        ];
        let msg = Message::mk_peers(peers.clone());

        assert_eq!(msg, Message::Peers { peers });
    }

    #[test]
    fn test_mk_getpeers() {
        let msg = Message::mk_getpeers();
        assert_eq!(msg, Message::GetPeers);
    }

    #[test]
    fn test_mk_error() {
        let name = "INVALID_HANDSHAKE".to_string();
        let msg_str = "peer did not send hello".to_string();
        let msg = Message::mk_error(name.clone(), msg_str.clone());

        assert_eq!(msg, Message::Error { name, msg: msg_str });
    }
}
