use serde::{Deserialize, Serialize};

use crate::protocol::peerlist::PeerAddr;
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(tag = "type")]
pub enum Message {
    Hello { port: u16, user_agent: String },
    Peers { peers: Vec<PeerAddr> },
    GetPeers,
    Error { name: String, msg: String },
}

impl Message {
    pub fn mk_hello(port: u16, user_agent: String) -> Self {
        Self::Hello { port, user_agent }
    }

    pub fn mk_peers(peers: Vec<PeerAddr>) -> Self {
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
        let peer1 = PeerAddr { host: "127.0.0.1".to_string(), port: 18018 };
        let peer2 = PeerAddr { host: "node.peer.com".to_string(), port: 18018 };
        let peers: Vec<PeerAddr> = vec![peer1, peer2];
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
}
