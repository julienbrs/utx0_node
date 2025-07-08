use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::util::constants::RECV_BUFFER_LIMIT;
use crate::{error::ProtocolError, protocol::message::Message};

pub async fn read_frame<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Message, ProtocolError> {
    let mut limited_reader = reader.take(RECV_BUFFER_LIMIT as u64);
    let mut line = String::new();
    let n = limited_reader.read_line(&mut line).await.map_err(ProtocolError::Io)?;

    if n >= RECV_BUFFER_LIMIT {
        return Err(ProtocolError::OversizedFrame); // frame too big
    }

    if line.trim().is_empty() {
        return Err(ProtocolError::ConnectionClosed); //EOF
    }

    // TODO: check when the line set is only containing /n (heartbeat)

    let message = serde_json::from_str(&line).map_err(|_| ProtocolError::InvalidFormat);
    return message;
}

pub async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    msg: &Message,
) -> Result<(), ProtocolError> {
    let mut line = serde_json::to_string(msg).map_err(|_| ProtocolError::InvalidFormat)?;
    line.push('\n'); // NDJSON format
    writer.write_all(line.as_bytes()).await.map_err(ProtocolError::Io)?;
    writer.flush().await.map_err(ProtocolError::Io)?;

    Ok(())
}

#[cfg(test)]
mod tests {

    use super::*;
    use tokio::io::{BufReader, BufWriter};

    #[tokio::test]
    async fn test_read_frame() {
        let buffer_size = RECV_BUFFER_LIMIT;
        let (client, mut server) = tokio::io::duplex(buffer_size);

        let msg = Message::GetPeers;
        let mut reader = BufReader::new(client);

        let mut json = serde_json::to_string(&msg).expect("serialization error");
        json.push('\n'); // in write_frame
        server.write_all(json.as_bytes()).await.expect("Io error while writing");

        let msg_read = read_frame(&mut reader).await.expect("Io error while reading");
        assert_eq!(msg_read, msg);
    }

    #[tokio::test]
    async fn test_write_frame() {
        let buffer_size = RECV_BUFFER_LIMIT;
        let (client, server) = tokio::io::duplex(buffer_size);
        let mut writer = BufWriter::new(server);
        let mut reader = BufReader::new(client);

        let msg = Message::GetPeers;
        write_frame(&mut writer, &msg).await.expect("Failed to write frame");

        let mut line = String::new();
        reader.read_line(&mut line).await.expect("Error reading line");

        let parsed = serde_json::from_str::<Message>(&line).expect("Failed to parse JSON");
        assert_eq!(parsed, msg);
    }

    #[tokio::test]
    async fn oversized_frame_rejected() {
        let (client, mut server) = tokio::io::duplex(600_000);
        let big = "A".repeat(RECV_BUFFER_LIMIT + 1) + "\n";
        server.write_all(big.as_bytes()).await.unwrap();

        let mut reader = BufReader::new(client);
        let res = read_frame(&mut reader).await;
        assert!(matches!(res, Err(ProtocolError::OversizedFrame)));
    }
}
