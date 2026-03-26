//! SO_PEERCRED-based authentication for Unix domain socket connections.
//!
//! Uses `tokio::net::unix::UCred` to retrieve the UID of a connected peer
//! and authorizes callers that are either root (UID 0) or the current
//! effective UID of the daemon process.

use tokio::net::UnixStream;

/// Retrieve the UID of the peer connected to a Unix stream via `SO_PEERCRED`.
///
/// Returns `None` if the credential lookup fails (e.g., unsupported platform).
pub fn get_peer_uid(stream: &UnixStream) -> Option<u32> {
    stream.peer_cred().ok().map(|cred| cred.uid())
}

/// Returns `true` if the given UID is authorized to issue control commands.
///
/// Authorization is granted to:
/// - **root** (UID 0) — always allowed.
/// - The current **effective UID** of this process — allows the user who
///   started the daemon to control it without privilege escalation.
pub fn authorize_local(uid: u32) -> bool {
    if uid == 0 {
        return true;
    }
    // Safety: getuid is always safe to call.
    let euid = unsafe { libc::geteuid() } as u32;
    uid == euid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_is_always_authorized() {
        assert!(authorize_local(0));
    }

    #[test]
    fn current_euid_is_authorized() {
        let euid = unsafe { libc::geteuid() } as u32;
        assert!(authorize_local(euid));
    }

    #[test]
    fn random_uid_is_rejected() {
        // UID 99999 is extremely unlikely to match root or current euid.
        // Skip if we happen to be running as that UID (essentially impossible).
        let euid = unsafe { libc::geteuid() } as u32;
        if euid != 99999 {
            assert!(!authorize_local(99999));
        }
    }

    #[tokio::test]
    async fn get_peer_uid_returns_some_on_unix_pair() {
        let dir = tempfile::tempdir().unwrap();
        let sock_path = dir.path().join("test.sock");
        let listener = tokio::net::UnixListener::bind(&sock_path).unwrap();

        let _client = tokio::net::UnixStream::connect(&sock_path).await.unwrap();
        let (server_stream, _) = listener.accept().await.unwrap();

        let uid = get_peer_uid(&server_stream);
        assert!(uid.is_some(), "should retrieve peer UID via SO_PEERCRED");
        let euid = unsafe { libc::geteuid() } as u32;
        assert_eq!(uid.unwrap(), euid);
    }
}
