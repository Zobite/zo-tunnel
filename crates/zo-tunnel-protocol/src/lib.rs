//! Zo Tunnel Protocol — Message definitions and async reader/writer.
//!
//! Binary frame format:
//! ```text
//! ┌──────────┬──────────┬───────────┬──────────────────┐
//! │ Version  │  Type    │  Length   │     Payload      │
//! │ (1 byte) │ (1 byte) │ (4 bytes) │  (N bytes)       │
//! └──────────┴──────────┴───────────┴──────────────────┘
//! ```

pub mod self_update;

use anyhow::{bail, Context, Result};
use bytes::{Buf, BufMut, BytesMut};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Protocol version
pub const PROTOCOL_VERSION: u8 = 1;

/// Maximum payload size (16 MB)
pub const MAX_PAYLOAD_SIZE: u32 = 16 * 1024 * 1024;

/// Default control port
pub const DEFAULT_CONTROL_PORT: u16 = 6200;

/// Default public port (HTTP subdomain routing + dashboard)
pub const DEFAULT_PUBLIC_PORT: u16 = 6210;

/// Heartbeat interval in seconds
pub const HEARTBEAT_INTERVAL_SECS: u64 = 15;

/// Heartbeat timeout in seconds (5 missed heartbeats)
pub const HEARTBEAT_TIMEOUT_SECS: u64 = 90;

/// Stream type markers for yamux streams
pub const STREAM_TYPE_PROXY: u8 = 0x00;
pub const STREAM_TYPE_HEARTBEAT: u8 = 0x01;

// ─── Message Types ───────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageType {
    AuthReq = 0x01,
    AuthRes = 0x02,
    NewConn = 0x03,
    Data = 0x04,
    Ping = 0x05,
    Pong = 0x06,
    Close = 0x07,
    Error = 0x08,
    AcceptConn = 0x09,
}

impl TryFrom<u8> for MessageType {
    type Error = anyhow::Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0x01 => Ok(Self::AuthReq),
            0x02 => Ok(Self::AuthRes),
            0x03 => Ok(Self::NewConn),
            0x04 => Ok(Self::Data),
            0x05 => Ok(Self::Ping),
            0x06 => Ok(Self::Pong),
            0x07 => Ok(Self::Close),
            0x08 => Ok(Self::Error),
            0x09 => Ok(Self::AcceptConn),
            _ => bail!("Unknown message type: 0x{:02x}", value),
        }
    }
}

// ─── Message Payloads ────────────────────────────────────────────

/// Authentication request from client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthReq {
    pub client_id: String,
    pub token: String,
    #[serde(default)]
    pub version: Option<String>,
}

impl AuthReq {
    /// Check if the client version supports heartbeat and stream type markers.
    /// Legacy clients do not send a version field.
    pub fn supports_heartbeat(&self) -> bool {
        self.version.is_some()
    }
}

/// Authentication response from server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthRes {
    pub success: bool,
    pub message: String,
    /// The public HTTP port
    pub public_port: Option<u16>,
    /// Assigned subdomain route (e.g. "my-app")
    pub assigned_route: Option<String>,
}

/// Server notifying client of a new incoming connection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewConn {
    pub conn_id: uuid::Uuid,
}

/// Client accepting a connection (sent on a new TCP stream)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceptConn {
    pub conn_id: uuid::Uuid,
    pub client_id: String,
}

// ─── Unified Message Enum ────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Message {
    AuthReq(AuthReq),
    AuthRes(AuthRes),
    NewConn(NewConn),
    AcceptConn(AcceptConn),
    Ping,
    Pong,
    Close,
    Error(String),
}

impl Message {
    /// Get the message type byte
    fn msg_type(&self) -> MessageType {
        match self {
            Message::AuthReq(_) => MessageType::AuthReq,
            Message::AuthRes(_) => MessageType::AuthRes,
            Message::NewConn(_) => MessageType::NewConn,
            Message::AcceptConn(_) => MessageType::AcceptConn,
            Message::Ping => MessageType::Ping,
            Message::Pong => MessageType::Pong,
            Message::Close => MessageType::Close,
            Message::Error(_) => MessageType::Error,
        }
    }

    /// Serialize the payload to bytes
    fn encode_payload(&self) -> Result<Vec<u8>> {
        match self {
            Message::AuthReq(p) => serde_json::to_vec(p).context("encode AuthReq"),
            Message::AuthRes(p) => serde_json::to_vec(p).context("encode AuthRes"),
            Message::NewConn(p) => serde_json::to_vec(p).context("encode NewConn"),
            Message::AcceptConn(p) => serde_json::to_vec(p).context("encode AcceptConn"),
            Message::Error(msg) => Ok(msg.as_bytes().to_vec()),
            Message::Ping | Message::Pong | Message::Close => Ok(vec![]),
        }
    }

    /// Deserialize from type + payload bytes
    fn decode(msg_type: MessageType, payload: &[u8]) -> Result<Self> {
        match msg_type {
            MessageType::AuthReq => {
                let p: AuthReq = serde_json::from_slice(payload).context("decode AuthReq")?;
                Ok(Message::AuthReq(p))
            }
            MessageType::AuthRes => {
                let p: AuthRes = serde_json::from_slice(payload).context("decode AuthRes")?;
                Ok(Message::AuthRes(p))
            }
            MessageType::NewConn => {
                let p: NewConn = serde_json::from_slice(payload).context("decode NewConn")?;
                Ok(Message::NewConn(p))
            }
            MessageType::AcceptConn => {
                let p: AcceptConn = serde_json::from_slice(payload).context("decode AcceptConn")?;
                Ok(Message::AcceptConn(p))
            }
            MessageType::Error => {
                let msg = String::from_utf8_lossy(payload).to_string();
                Ok(Message::Error(msg))
            }
            MessageType::Ping => Ok(Message::Ping),
            MessageType::Pong => Ok(Message::Pong),
            MessageType::Close => Ok(Message::Close),
            MessageType::Data => bail!("DATA messages should not go through Message::decode"),
        }
    }
}

// ─── Async Writer ────────────────────────────────────────────────

/// Write a protocol message to an async writer
pub async fn write_message<W: AsyncWrite + Unpin>(writer: &mut W, msg: &Message) -> Result<()> {
    let payload = msg.encode_payload()?;
    let payload_len = payload.len() as u32;

    if payload_len > MAX_PAYLOAD_SIZE {
        bail!(
            "Payload too large: {} bytes (max {})",
            payload_len,
            MAX_PAYLOAD_SIZE
        );
    }

    let mut buf = BytesMut::with_capacity(6 + payload.len());
    buf.put_u8(PROTOCOL_VERSION);
    buf.put_u8(msg.msg_type() as u8);
    buf.put_u32(payload_len);
    buf.put_slice(&payload);

    writer.write_all(&buf).await.context("write message")?;
    writer.flush().await.context("flush message")?;
    Ok(())
}

/// Read a protocol message from an async reader
pub async fn read_message<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Message> {
    // Read header: version (1) + type (1) + length (4) = 6 bytes
    let mut header = [0u8; 6];
    reader
        .read_exact(&mut header)
        .await
        .context("read message header")?;

    let mut buf = &header[..];
    let version = buf.get_u8();
    let msg_type_byte = buf.get_u8();
    let payload_len = buf.get_u32();

    if version != PROTOCOL_VERSION {
        bail!(
            "Protocol version mismatch: got {}, expected {}",
            version,
            PROTOCOL_VERSION
        );
    }

    let msg_type = MessageType::try_from(msg_type_byte)?;

    if payload_len > MAX_PAYLOAD_SIZE {
        bail!(
            "Payload too large: {} bytes (max {})",
            payload_len,
            MAX_PAYLOAD_SIZE
        );
    }

    // Read payload
    let mut payload = vec![0u8; payload_len as usize];
    if payload_len > 0 {
        reader
            .read_exact(&mut payload)
            .await
            .context("read message payload")?;
    }

    Message::decode(msg_type, &payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    #[tokio::test]
    async fn test_roundtrip_auth_req() {
        let (mut client, mut server) = duplex(1024);

        let msg = Message::AuthReq(AuthReq {
            client_id: "test-client".into(),
            token: "secret123".into(),
            version: None,
        });

        write_message(&mut client, &msg).await.unwrap();
        let received = read_message(&mut server).await.unwrap();

        if let Message::AuthReq(auth) = received {
            assert_eq!(auth.client_id, "test-client");
            assert_eq!(auth.token, "secret123");
        } else {
            panic!("Expected AuthReq, got {:?}", received);
        }
    }

    #[tokio::test]
    async fn test_roundtrip_auth_res() {
        let (mut client, mut server) = duplex(1024);

        let msg = Message::AuthRes(AuthRes {
            success: true,
            message: "OK".into(),
            public_port: Some(6210),
            assigned_route: Some("my-app".into()),
        });

        write_message(&mut client, &msg).await.unwrap();
        let received = read_message(&mut server).await.unwrap();

        if let Message::AuthRes(res) = received {
            assert!(res.success);
            assert_eq!(res.assigned_route, Some("my-app".into()));
        } else {
            panic!("Expected AuthRes, got {:?}", received);
        }
    }

    #[tokio::test]
    async fn test_roundtrip_ping_pong() {
        let (mut client, mut server) = duplex(1024);

        write_message(&mut client, &Message::Ping).await.unwrap();
        let received = read_message(&mut server).await.unwrap();
        assert!(matches!(received, Message::Ping));

        write_message(&mut server, &Message::Pong).await.unwrap();
        let received = read_message(&mut client).await.unwrap();
        assert!(matches!(received, Message::Pong));
    }

    #[tokio::test]
    async fn test_roundtrip_new_conn() {
        let (mut client, mut server) = duplex(1024);

        let conn_id = uuid::Uuid::new_v4();
        let msg = Message::NewConn(NewConn { conn_id });

        write_message(&mut server, &msg).await.unwrap();
        let received = read_message(&mut client).await.unwrap();

        if let Message::NewConn(nc) = received {
            assert_eq!(nc.conn_id, conn_id);
        } else {
            panic!("Expected NewConn, got {:?}", received);
        }
    }

    #[tokio::test]
    async fn test_auth_res_with_none_fields() {
        let (mut client, mut server) = duplex(1024);

        let msg = Message::AuthRes(AuthRes {
            success: false,
            message: "denied".into(),
            public_port: None,
            assigned_route: None,
        });

        write_message(&mut client, &msg).await.unwrap();
        let received = read_message(&mut server).await.unwrap();

        if let Message::AuthRes(res) = received {
            assert!(!res.success);
            assert_eq!(res.message, "denied");
            assert!(res.public_port.is_none());
            assert!(res.assigned_route.is_none());
        } else {
            panic!("Expected AuthRes, got {:?}", received);
        }
    }

    #[tokio::test]
    async fn test_read_from_closed_connection() {
        let (client, mut server) = duplex(1024);
        drop(client); // close the writing end

        let result = read_message(&mut server).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_large_client_id() {
        let (mut client, mut server) = duplex(64 * 1024);

        let long_id = "a".repeat(1000);
        let msg = Message::AuthReq(AuthReq {
            client_id: long_id.clone(),
            token: "t".into(),
            version: None,
        });

        write_message(&mut client, &msg).await.unwrap();
        let received = read_message(&mut server).await.unwrap();

        if let Message::AuthReq(auth) = received {
            assert_eq!(auth.client_id, long_id);
        } else {
            panic!("Expected AuthReq");
        }
    }

    #[test]
    fn test_constants() {
        assert_eq!(PROTOCOL_VERSION, 1);
        assert_eq!(MAX_PAYLOAD_SIZE, 16 * 1024 * 1024);
        assert_eq!(DEFAULT_CONTROL_PORT, 6200);
        assert_eq!(DEFAULT_PUBLIC_PORT, 6210);
        assert_eq!(HEARTBEAT_INTERVAL_SECS, 15);
        assert_eq!(HEARTBEAT_TIMEOUT_SECS, 90);
    }
}
