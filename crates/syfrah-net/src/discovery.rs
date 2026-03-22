use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use syfrah_core::mesh::{decrypt_record, encrypt_record, PeerRecord};
use syfrah_core::secret::MeshSecret;

const DEFAULT_IPFS_API: &str = "http://127.0.0.1:5001";
const PUBLISH_INTERVAL: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("IPFS API error: {0}")]
    Ipfs(String),
    #[error("encryption error: {0}")]
    Encryption(#[from] syfrah_core::mesh::MeshError),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// IPFS-based peer discovery.
/// Each node publishes its encrypted PeerRecord to IPFS under a key derived from the mesh secret.
/// All nodes poll that key to discover peers.
pub struct IpfsDiscovery {
    client: Client,
    ipfs_api: String,
    mesh_secret: MeshSecret,
    peers: Arc<RwLock<Vec<PeerRecord>>>,
}

impl IpfsDiscovery {
    pub fn new(mesh_secret: MeshSecret, ipfs_api: Option<String>) -> Self {
        Self {
            client: Client::new(),
            ipfs_api: ipfs_api.unwrap_or_else(|| DEFAULT_IPFS_API.to_string()),
            mesh_secret,
            peers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn peers(&self) -> Arc<RwLock<Vec<PeerRecord>>> {
        self.peers.clone()
    }

    /// Publish our PeerRecord to IPFS.
    /// We publish an encrypted JSON file to IPFS MFS (Mutable File System) under a path
    /// derived from the mesh secret + our WG pubkey.
    pub async fn publish(&self, record: &PeerRecord) -> Result<(), DiscoveryError> {
        let mesh_key = self.mesh_secret.ipfs_key_hex();
        let node_key = sha256_hex(&record.wg_public_key);

        // Encrypt the record
        let encrypted = encrypt_record(record, &self.mesh_secret.encryption_key())?;
        let encoded = data_encoding::BASE64.encode(&encrypted);

        // Write to IPFS MFS: /syfrah/{mesh_key}/{node_key}
        let dir_path = format!("/syfrah/{mesh_key}");
        let file_path = format!("{dir_path}/{node_key}");

        // Ensure directory exists
        let _ = self.ipfs_mfs_mkdir(&dir_path).await;

        // Write the file
        self.ipfs_mfs_write(&file_path, encoded.as_bytes()).await?;

        debug!("published peer record for {} to IPFS", record.name);
        Ok(())
    }

    /// Read all peer records from IPFS for this mesh.
    pub async fn discover(&self) -> Result<Vec<PeerRecord>, DiscoveryError> {
        let mesh_key = self.mesh_secret.ipfs_key_hex();
        let dir_path = format!("/syfrah/{mesh_key}");
        let enc_key = self.mesh_secret.encryption_key();

        // List files in the directory
        let entries = match self.ipfs_mfs_ls(&dir_path).await {
            Ok(e) => e,
            Err(_) => return Ok(vec![]), // directory doesn't exist yet
        };

        let mut records = Vec::new();
        for entry_name in &entries {
            let file_path = format!("{dir_path}/{entry_name}");
            match self.ipfs_mfs_read(&file_path).await {
                Ok(data) => {
                    match data_encoding::BASE64.decode(data.as_bytes()) {
                        Ok(encrypted) => {
                            match decrypt_record(&encrypted, &enc_key) {
                                Ok(record) => records.push(record),
                                Err(e) => debug!("failed to decrypt record {entry_name}: {e}"),
                            }
                        }
                        Err(e) => debug!("invalid base64 in {entry_name}: {e}"),
                    }
                }
                Err(e) => debug!("failed to read {file_path}: {e}"),
            }
        }

        Ok(records)
    }

    /// Run the discovery loop: periodically publish our record and poll for peers.
    /// Calls `on_change` when the peer list changes.
    pub async fn run(
        &self,
        my_record: PeerRecord,
        on_change: Arc<dyn Fn(&PeerRecord) + Send + Sync>,
    ) -> Result<(), DiscoveryError> {
        let mut publish_interval = tokio::time::interval(PUBLISH_INTERVAL);
        let mut poll_interval = tokio::time::interval(POLL_INTERVAL);

        loop {
            tokio::select! {
                _ = publish_interval.tick() => {
                    let mut record = my_record.clone();
                    record.last_seen = now();
                    if let Err(e) = self.publish(&record).await {
                        warn!("IPFS publish failed: {e}");
                    }
                }
                _ = poll_interval.tick() => {
                    match self.discover().await {
                        Ok(records) => {
                            let mut peers = self.peers.write().await;
                            for record in records {
                                let existing = peers.iter_mut()
                                    .find(|p| p.wg_public_key == record.wg_public_key);
                                match existing {
                                    Some(p) => {
                                        if record.last_seen > p.last_seen {
                                            *p = record.clone();
                                            on_change(&record);
                                        }
                                    }
                                    None => {
                                        info!("discovered new peer: {} ({})", record.name, record.mesh_ipv6);
                                        on_change(&record);
                                        peers.push(record);
                                    }
                                }
                            }
                        }
                        Err(e) => warn!("IPFS discover failed: {e}"),
                    }
                }
            }
        }
    }

    // --- IPFS MFS API helpers ---

    async fn ipfs_mfs_mkdir(&self, path: &str) -> Result<(), DiscoveryError> {
        let url = format!("{}/api/v0/files/mkdir", self.ipfs_api);
        self.client
            .post(&url)
            .query(&[("arg", path), ("parents", "true")])
            .send()
            .await?
            .error_for_status()
            .map_err(|e| DiscoveryError::Ipfs(e.to_string()))?;
        Ok(())
    }

    async fn ipfs_mfs_write(&self, path: &str, data: &[u8]) -> Result<(), DiscoveryError> {
        let url = format!("{}/api/v0/files/write", self.ipfs_api);
        let form = reqwest::multipart::Form::new()
            .part("file", reqwest::multipart::Part::bytes(data.to_vec()));
        self.client
            .post(&url)
            .query(&[("arg", path), ("create", "true"), ("truncate", "true")])
            .multipart(form)
            .send()
            .await?
            .error_for_status()
            .map_err(|e| DiscoveryError::Ipfs(e.to_string()))?;
        Ok(())
    }

    async fn ipfs_mfs_read(&self, path: &str) -> Result<String, DiscoveryError> {
        let url = format!("{}/api/v0/files/read", self.ipfs_api);
        let resp = self.client
            .post(&url)
            .query(&[("arg", path)])
            .send()
            .await?
            .error_for_status()
            .map_err(|e| DiscoveryError::Ipfs(e.to_string()))?;
        let text = resp.text().await?;
        Ok(text)
    }

    async fn ipfs_mfs_ls(&self, path: &str) -> Result<Vec<String>, DiscoveryError> {
        let url = format!("{}/api/v0/files/ls", self.ipfs_api);
        let resp = self.client
            .post(&url)
            .query(&[("arg", path), ("long", "false")])
            .send()
            .await?
            .error_for_status()
            .map_err(|e| DiscoveryError::Ipfs(e.to_string()))?;
        let body: serde_json::Value = resp.json().await?;
        let entries = body["Entries"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|e| e["Name"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        Ok(entries)
    }
}

fn sha256_hex(input: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(input.as_bytes());
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
