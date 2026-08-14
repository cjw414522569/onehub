use std::time::Duration;

use ed25519_dalek::Signer;
use session_orchestrator::cancellation::{select_deadline, Deadline};

/// SSH agent protocol message codes (RFC 4252 §7 and OpenSSH PROTOCOL.agent).
pub mod msg {
    /// Request the list of identities.
    pub const REQUEST_IDENTITIES: u8 = 11;
    /// Response with the identity list.
    pub const IDENTITIES_ANSWER: u8 = 12;
    /// Request a signature.
    pub const SIGN_REQUEST: u8 = 13;
    /// Signature response.
    pub const SIGN_RESPONSE: u8 = 14;
    /// Operation failure.
    pub const FAILURE: u8 = 5;
    /// Operation success.
    pub const SUCCESS: u8 = 6;
    /// OpenSSH extension.
    pub const EXTENSION: u8 = 27;
}

/// Sign request flag: the agent must ask the user for confirmation.
pub const SSH_AGENT_CONSTRAIN_CONFIRM: u32 = 1;

/// Maximum agent message size.
pub const AGENT_MAX_MESSAGE_LEN: u32 = 256 * 1024;

/// Agent protocol error (no secret context).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentError {
    /// The agent returned a failure.
    Failure,
    /// The peer disconnected (EOF).
    Disconnected,
    /// The operation timed out.
    Timeout,
    /// The message framing or content was invalid.
    Protocol,
    /// The message is too large.
    TooLarge,
}

/// Frames a payload as `u32 length || payload`.
pub fn frame_message(payload: &[u8]) -> Vec<u8> {
    let mut framed = Vec::with_capacity(4 + payload.len());
    framed.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    framed.extend_from_slice(payload);
    framed
}

/// Parses one framed message from a buffer.
///
/// Returns `Ok((payload, consumed))` when a full message is present,
/// `Ok(None)` when more bytes are needed, or an error for invalid framing.
pub fn parse_frame(buffer: &[u8]) -> Result<Option<(Vec<u8>, usize)>, AgentError> {
    if buffer.len() < 4 {
        return Ok(None);
    }
    let length = u32::from_be_bytes(buffer[0..4].try_into().expect("4 bytes")) as usize;
    if length > AGENT_MAX_MESSAGE_LEN as usize {
        return Err(AgentError::TooLarge);
    }
    if buffer.len() < 4 + length {
        return Ok(None);
    }
    Ok(Some((buffer[4..4 + length].to_vec(), 4 + length)))
}

/// An identity offered by the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentIdentity {
    /// SSH public key blob.
    pub key_blob: Vec<u8>,
    /// Comment.
    pub comment: String,
}

/// A duplex byte stream to the agent (a real socket adapter implements this).
pub trait AgentStream: Send + Sync {
    /// Writes all bytes.
    fn write_all(
        &self,
        bytes: &[u8],
    ) -> impl std::future::Future<Output = Result<(), AgentError>> + Send;
    /// Reads exactly `buf.len()` bytes; EOF maps to `Disconnected`.
    fn read_exact(
        &self,
        buf: &mut [u8],
    ) -> impl std::future::Future<Output = Result<(), AgentError>> + Send;
}

/// Agent client with identity enumeration, signing, timeout, and disconnect
/// handling.
pub struct AgentClient<S> {
    /// The underlying stream.
    pub stream: S,
    /// Per-operation timeout.
    pub timeout: Duration,
}

impl<S: AgentStream> AgentClient<S> {
    /// Creates a client.
    pub fn new(stream: S, timeout: Duration) -> Self {
        Self { stream, timeout }
    }

    /// Enumerates identities.
    pub async fn request_identities(&self) -> Result<Vec<AgentIdentity>, AgentError> {
        let payload = self.round_trip(&[msg::REQUEST_IDENTITIES]).await?;
        if payload.is_empty() || payload[0] != msg::IDENTITIES_ANSWER {
            return Err(AgentError::Failure);
        }
        let mut cursor = 1usize;
        let count = read_u32(&payload, &mut cursor)?;
        let mut identities = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let key_blob = read_string(&payload, &mut cursor)?;
            let comment = read_string(&payload, &mut cursor)?;
            identities.push(AgentIdentity {
                key_blob,
                comment: String::from_utf8_lossy(&comment).into_owned(),
            });
        }
        Ok(identities)
    }

    /// Requests a signature over `data` for the key identified by
    /// `key_blob`. The `confirm` flag asks the agent to require user
    /// confirmation.
    pub async fn sign(
        &self,
        key_blob: &[u8],
        data: &[u8],
        confirm: bool,
    ) -> Result<Vec<u8>, AgentError> {
        let mut payload = Vec::with_capacity(1 + 4 + key_blob.len() + 4 + data.len() + 4);
        payload.push(msg::SIGN_REQUEST);
        write_string(&mut payload, key_blob);
        write_string(&mut payload, data);
        let flags = if confirm {
            SSH_AGENT_CONSTRAIN_CONFIRM
        } else {
            0
        };
        payload.extend_from_slice(&flags.to_be_bytes());

        let response = self.round_trip(&payload).await?;
        if response.is_empty() || response[0] != msg::SIGN_RESPONSE {
            return Err(AgentError::Failure);
        }
        let mut cursor = 1usize;
        read_string(&response, &mut cursor)
    }

    async fn round_trip(&self, payload: &[u8]) -> Result<Vec<u8>, AgentError> {
        let framed = frame_message(payload);
        let operation = async {
            self.stream.write_all(&framed).await?;
            let mut length_bytes = [0u8; 4];
            self.stream.read_exact(&mut length_bytes).await?;
            let length = u32::from_be_bytes(length_bytes) as usize;
            if length > AGENT_MAX_MESSAGE_LEN as usize {
                return Err(AgentError::TooLarge);
            }
            let mut body = vec![0u8; length];
            self.stream.read_exact(&mut body).await?;
            Ok(body)
        };
        let deadline = Deadline::after(self.timeout);
        match select_deadline(deadline, operation).await {
            Ok(Ok(body)) => Ok(body),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(AgentError::Timeout),
        }
    }
}

/// An in-memory fake agent server implementing the protocol (for tests and
/// contract verification).
pub struct FakeAgentServer {
    /// Identities offered by the agent.
    pub identities: Vec<AgentIdentity>,
    /// Ed25519 signing keys keyed by their 32-byte public key.
    pub signing_keys: Vec<ed25519_dalek::SigningKey>,
    /// Whether sign requests require confirmation (flag is propagated).
    pub require_confirm: bool,
}

impl FakeAgentServer {
    /// Creates a server with the given identities and keys.
    pub fn new(
        identities: Vec<AgentIdentity>,
        signing_keys: Vec<ed25519_dalek::SigningKey>,
    ) -> Self {
        Self {
            identities,
            signing_keys,
            require_confirm: false,
        }
    }

    /// Handles one framed request payload and returns the framed response.
    pub fn handle(&self, framed: &[u8]) -> Vec<u8> {
        let Ok(Some((payload, _))) = parse_frame(framed) else {
            return frame_message(&[msg::FAILURE]);
        };
        if payload.is_empty() {
            return frame_message(&[msg::FAILURE]);
        }
        match payload[0] {
            msg::REQUEST_IDENTITIES => {
                let mut body = vec![msg::IDENTITIES_ANSWER];
                body.extend_from_slice(&(self.identities.len() as u32).to_be_bytes());
                for identity in &self.identities {
                    write_string(&mut body, &identity.key_blob);
                    write_string(&mut body, identity.comment.as_bytes());
                }
                frame_message(&body)
            }
            msg::SIGN_REQUEST => {
                let mut cursor = 1usize;
                let Ok(key_blob) = read_string(&payload, &mut cursor) else {
                    return frame_message(&[msg::FAILURE]);
                };
                let Ok(data) = read_string(&payload, &mut cursor) else {
                    return frame_message(&[msg::FAILURE]);
                };
                let Ok(flags) = read_u32_opt(&payload, &mut cursor) else {
                    return frame_message(&[msg::FAILURE]);
                };
                let _confirm_requested =
                    self.require_confirm && flags & SSH_AGENT_CONSTRAIN_CONFIRM != 0;
                // The key blob is `ssh-ed25519` || len(32) || key; the key is the
                // final 32 bytes.
                let Some(key_bytes) = key_blob.get(key_blob.len().saturating_sub(32)..) else {
                    return frame_message(&[msg::FAILURE]);
                };
                let Some(signing_key) = self
                    .signing_keys
                    .iter()
                    .find(|key| key.verifying_key().to_bytes().as_slice() == key_bytes)
                else {
                    return frame_message(&[msg::FAILURE]);
                };
                let signature = signing_key.sign(&data);
                let mut body = vec![msg::SIGN_RESPONSE];
                write_string(&mut body, &signature.to_bytes());
                frame_message(&body)
            }
            _ => frame_message(&[msg::FAILURE]),
        }
    }
}

fn write_string(buffer: &mut Vec<u8>, value: &[u8]) {
    buffer.extend_from_slice(&(value.len() as u32).to_be_bytes());
    buffer.extend_from_slice(value);
}

fn read_u32(buffer: &[u8], cursor: &mut usize) -> Result<u32, AgentError> {
    if *cursor + 4 > buffer.len() {
        return Err(AgentError::Protocol);
    }
    let value = u32::from_be_bytes(buffer[*cursor..*cursor + 4].try_into().expect("4 bytes"));
    *cursor += 4;
    Ok(value)
}

fn read_u32_opt(buffer: &[u8], cursor: &mut usize) -> Result<u32, AgentError> {
    read_u32(buffer, cursor)
}

fn read_string(buffer: &[u8], cursor: &mut usize) -> Result<Vec<u8>, AgentError> {
    let length = read_u32(buffer, cursor)? as usize;
    if *cursor + length > buffer.len() {
        return Err(AgentError::Protocol);
    }
    let value = buffer[*cursor..*cursor + length].to_vec();
    *cursor += length;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::{
        frame_message, msg, parse_frame, AgentClient, AgentError, AgentIdentity, FakeAgentServer,
    };
    use ed25519_dalek::{SigningKey, Verifier};
    use rand::rngs::OsRng;
    use std::time::Duration;

    fn public_blob(key: &SigningKey) -> Vec<u8> {
        // `ssh-ed25519` blob.
        let name = b"ssh-ed25519";
        let mut blob = Vec::new();
        blob.extend_from_slice(&(name.len() as u32).to_be_bytes());
        blob.extend_from_slice(name);
        blob.extend_from_slice(&(32u32).to_be_bytes());
        blob.extend_from_slice(&key.verifying_key().to_bytes());
        blob
    }

    /// A duplex stream over tokio in-memory pipes; the fake server runs on the
    /// other end.
    struct DuplexAgent {
        write: tokio::sync::Mutex<tokio::io::DuplexStream>,
        read: tokio::sync::Mutex<tokio::io::DuplexStream>,
    }

    impl super::AgentStream for DuplexAgent {
        async fn write_all(&self, bytes: &[u8]) -> Result<(), AgentError> {
            use tokio::io::AsyncWriteExt;
            self.write
                .lock()
                .await
                .write_all(bytes)
                .await
                .map_err(|_| AgentError::Disconnected)
        }
        async fn read_exact(&self, buf: &mut [u8]) -> Result<(), AgentError> {
            use tokio::io::AsyncReadExt;
            match self.read.lock().await.read_exact(buf).await {
                Ok(_) => Ok(()),
                Err(_) => Err(AgentError::Disconnected),
            }
        }
    }

    async fn connect(server: &FakeAgentServer) -> DuplexAgent {
        let (client_write, server_read) = tokio::io::duplex(65536);
        let (server_write, client_read) = tokio::io::duplex(65536);
        // Drive the server in a background task.
        tokio::spawn({
            let server = server.clone_for_test();
            async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let (mut server_read, mut server_write) = (server_read, server_write);
                let mut buffer = Vec::new();
                loop {
                    let mut byte = [0u8; 1];
                    match server_read.read_exact(&mut byte).await {
                        Ok(_) => buffer.push(byte[0]),
                        Err(_) => break,
                    }
                    if let Ok(Some((_payload, consumed))) = parse_frame(&buffer) {
                        let response = server.handle(&buffer[..consumed]);
                        if server_write.write_all(&response).await.is_err() {
                            break;
                        }
                        buffer.drain(..consumed);
                    }
                }
            }
        });
        DuplexAgent {
            write: tokio::sync::Mutex::new(client_write),
            read: tokio::sync::Mutex::new(client_read),
        }
    }

    impl FakeAgentServer {
        fn clone_for_test(&self) -> FakeAgentServer {
            FakeAgentServer {
                identities: self.identities.clone(),
                signing_keys: self.signing_keys.clone(),
                require_confirm: self.require_confirm,
            }
        }
    }

    #[test]
    fn framing_round_trip() {
        let payload = vec![msg::REQUEST_IDENTITIES];
        let framed = frame_message(&payload);
        assert_eq!(framed.len(), 5);
        assert_eq!(&framed[..4], &[0, 0, 0, 1]);
        let (parsed, consumed) = parse_frame(&framed).expect("parse").expect("complete");
        assert_eq!(parsed, payload);
        assert_eq!(consumed, 5);
        // Incomplete input returns None.
        assert!(parse_frame(&framed[..4]).expect("no error").is_none());
        // Oversized framing is rejected.
        let mut oversized = vec![0u8; 4];
        oversized[..4].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(parse_frame(&oversized), Err(AgentError::TooLarge));
    }

    #[tokio::test]
    async fn identity_enumeration_over_duplex() {
        let key = SigningKey::generate(&mut OsRng);
        let identities = vec![AgentIdentity {
            key_blob: public_blob(&key),
            comment: "test@host".to_owned(),
        }];
        let server = FakeAgentServer::new(identities.clone(), vec![key.clone()]);
        let stream = connect(&server).await;
        let client = AgentClient::new(stream, Duration::from_secs(2));
        let listed = client.request_identities().await.expect("identities");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].key_blob, identities[0].key_blob);
        assert_eq!(listed[0].comment, "test@host");
    }

    #[tokio::test]
    async fn sign_request_returns_verifiable_signature() {
        let key = SigningKey::generate(&mut OsRng);
        let identities = vec![AgentIdentity {
            key_blob: public_blob(&key),
            comment: "sign-test".to_owned(),
        }];
        let server = FakeAgentServer::new(identities, vec![key.clone()]);
        let stream = connect(&server).await;
        let client = AgentClient::new(stream, Duration::from_secs(2));
        let data = b"challenge-data";
        let signature_bytes = client
            .sign(&public_blob(&key), data, false)
            .await
            .expect("sign");
        let signature = ed25519_dalek::Signature::from_slice(&signature_bytes).expect("signature");
        key.verifying_key()
            .verify(data, &signature)
            .expect("signature must verify");
    }

    #[tokio::test]
    async fn unknown_key_sign_returns_failure() {
        let key = SigningKey::generate(&mut OsRng);
        let other = SigningKey::generate(&mut OsRng);
        let server = FakeAgentServer::new(vec![], vec![key]);
        let stream = connect(&server).await;
        let client = AgentClient::new(stream, Duration::from_secs(2));
        assert_eq!(
            client.sign(&public_blob(&other), b"data", false).await,
            Err(AgentError::Failure)
        );
    }

    #[tokio::test]
    async fn timeout_is_enforced_when_server_is_silent() {
        // A server that keeps its ends open but never responds.
        let (client_write, server_read) = tokio::io::duplex(65536);
        let (server_write, client_read) = tokio::io::duplex(65536);
        let _keep_alive = (server_read, server_write);
        let stream = DuplexAgent {
            write: tokio::sync::Mutex::new(client_write),
            read: tokio::sync::Mutex::new(client_read),
        };
        let client = AgentClient::new(stream, Duration::from_millis(20));
        let start = std::time::Instant::now();
        let result = client.request_identities().await;
        assert_eq!(result, Err(AgentError::Timeout));
        assert!(start.elapsed() >= Duration::from_millis(15));
    }

    #[tokio::test]
    async fn disconnect_is_detected_as_eof() {
        // Server closes the read side immediately.
        let (client_write, server_read) = tokio::io::duplex(65536);
        let (server_write, client_read) = tokio::io::duplex(65536);
        drop(server_read);
        drop(server_write);
        let stream = DuplexAgent {
            write: tokio::sync::Mutex::new(client_write),
            read: tokio::sync::Mutex::new(client_read),
        };
        let client = AgentClient::new(stream, Duration::from_secs(2));
        assert_eq!(
            client.request_identities().await,
            Err(AgentError::Disconnected)
        );
    }
}
