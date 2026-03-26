use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tracing::warn;

/// Maximum message size: 64 KB.
pub const MAX_MESSAGE_SIZE: u32 = 65_536;

/// Default read timeout for control socket reads.
pub const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// Bind a Unix listener at `socket_path` with restrictive permissions (0o600).
///
/// Removes any stale socket file, sets umask to 0o177 before bind so the
/// socket is created owner-only, then restores the original umask.
pub fn bind_unix_listener(socket_path: &Path) -> Result<UnixListener, std::io::Error> {
    // Remove stale socket
    let _ = std::fs::remove_file(socket_path);

    // Set restrictive umask *before* bind to eliminate the permission race window.
    #[cfg(unix)]
    let old_umask = unsafe { libc::umask(0o177) };

    let listener = match UnixListener::bind(socket_path) {
        Ok(l) => {
            #[cfg(unix)]
            unsafe {
                libc::umask(old_umask);
            }
            l
        }
        Err(e) => {
            #[cfg(unix)]
            unsafe {
                libc::umask(old_umask);
            }
            warn!(
                "failed to bind control socket at {}: {e}",
                socket_path.display()
            );
            return Err(e);
        }
    };

    Ok(listener)
}

/// Write a length-prefixed JSON message to an async writer.
pub async fn write_message<T: Serialize, W: AsyncWriteExt + Unpin>(
    stream: &mut W,
    msg: &T,
) -> Result<(), Box<dyn std::error::Error>> {
    let data = serde_json::to_vec(msg)?;
    let len = data.len() as u32;
    stream.write_all(&len.to_be_bytes()).await?;
    stream.write_all(&data).await?;
    stream.flush().await?;
    Ok(())
}

/// Read a length-prefixed JSON message from an async reader.
/// Rejects messages larger than [`MAX_MESSAGE_SIZE`] bytes.
pub async fn read_message<T: serde::de::DeserializeOwned, R: AsyncReadExt + Unpin>(
    stream: &mut R,
) -> Result<T, Box<dyn std::error::Error>> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf);
    if len > MAX_MESSAGE_SIZE {
        return Err("control message too large".into());
    }
    let mut data = vec![0u8; len as usize];
    stream.read_exact(&mut data).await?;
    let msg: T = serde_json::from_slice(&data)?;
    Ok(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use tokio::io::duplex;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestMsg {
        value: String,
    }

    #[tokio::test]
    async fn message_roundtrip() {
        let (mut client, mut server) = duplex(4096);

        let msg = TestMsg {
            value: "hello".into(),
        };
        write_message(&mut client, &msg).await.unwrap();
        drop(client);

        let read_msg: TestMsg = read_message(&mut server).await.unwrap();
        assert_eq!(read_msg, msg);
    }

    #[tokio::test]
    async fn oversized_message_rejected() {
        let (mut client, mut server) = duplex(64);

        let fake_len: u32 = MAX_MESSAGE_SIZE + 1;
        tokio::io::AsyncWriteExt::write_all(&mut client, &fake_len.to_be_bytes())
            .await
            .unwrap();
        drop(client);

        let result: Result<TestMsg, _> = read_message(&mut server).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("too large"),
            "expected 'too large' error, got: {err_msg}"
        );
    }

    #[tokio::test]
    async fn malformed_json_rejected() {
        let (mut client, mut server) = duplex(4096);

        let bad_json = b"not valid json";
        let len = bad_json.len() as u32;
        tokio::io::AsyncWriteExt::write_all(&mut client, &len.to_be_bytes())
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut client, bad_json)
            .await
            .unwrap();
        drop(client);

        let result: Result<TestMsg, _> = read_message(&mut server).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn empty_stream_errors() {
        let (_client, mut server) = duplex(4096);
        drop(_client);

        let result: Result<TestMsg, _> = read_message(&mut server).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn truncated_body_errors() {
        let (mut client, mut server) = duplex(4096);

        let len: u32 = 100;
        tokio::io::AsyncWriteExt::write_all(&mut client, &len.to_be_bytes())
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut client, b"hello")
            .await
            .unwrap();
        drop(client);

        let result: Result<TestMsg, _> = read_message(&mut server).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn bind_unix_listener_creates_socket() {
        let dir = tempfile::tempdir().unwrap();
        let sock_path = dir.path().join("test.sock");
        let listener = bind_unix_listener(&sock_path).unwrap();
        drop(listener);
        // Socket file should exist
        assert!(sock_path.exists());
    }
}
