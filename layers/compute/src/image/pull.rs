use std::fs::{self, File};
use std::io::{Read, Write};

use flate2::read::GzDecoder;
use fs2::FileExt;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tracing::info;

use super::error::ImageError;
use super::store::ImageStore;
use super::types::{ImageCatalog, ImageMeta, RuntimeMode};

/// Pull an image from the catalog, streaming download + gzip decompress +
/// SHA256 verify. Idempotent: if the image already exists locally with the
/// same SHA, this is a no-op.
pub async fn pull(
    store: &ImageStore,
    name: &str,
    catalog: &ImageCatalog,
) -> Result<ImageMeta, ImageError> {
    // 1. Find image in catalog
    let catalog_entry = catalog
        .images
        .iter()
        .find(|i| i.name == name)
        .ok_or_else(|| ImageError::CatalogFetchFailed {
            url: catalog.base_url.clone(),
            reason: format!("image '{name}' not found in catalog"),
        })?;

    // 2. Check if already present with same SHA (idempotent)
    if let Some(existing) = store.get(name)? {
        if existing.sha256 == catalog_entry.sha256 {
            info!(
                name,
                "image already present with matching SHA, skipping pull"
            );
            return Ok(existing);
        }
        // Different SHA — will re-pull
        info!(name, "image present but SHA differs, re-pulling");
    }

    // 3. Acquire file lock
    let lock_path = store.image_dir().join(".lock");
    let lock_file = File::create(&lock_path).map_err(|e| ImageError::ImportFailed {
        reason: format!("failed to create lock file: {e}"),
    })?;
    lock_file
        .lock_exclusive()
        .map_err(|e| ImageError::ImportFailed {
            reason: format!("failed to acquire lock: {e}"),
        })?;

    let result = pull_inner(store, name, catalog_entry, &catalog.base_url).await;

    // Release lock (drop does it, but be explicit)
    let _ = lock_file.unlock();
    drop(lock_file);

    result
}

/// Pull an image from the catalog, selecting the right format for the given
/// runtime mode.
///
/// - [`RuntimeMode::Vm`]: downloads the `.raw.gz` variant (default behavior).
/// - [`RuntimeMode::Container`]: downloads the OCI `-oci.tar.gz` variant if
///   `container_file` is present in the catalog entry; falls back to the VM
///   variant if no container image is available.
pub async fn pull_for_runtime(
    store: &ImageStore,
    name: &str,
    catalog: &ImageCatalog,
    mode: &RuntimeMode,
) -> Result<ImageMeta, ImageError> {
    let catalog_entry = catalog
        .images
        .iter()
        .find(|i| i.name == name)
        .ok_or_else(|| ImageError::CatalogFetchFailed {
            url: catalog.base_url.clone(),
            reason: format!("image '{name}' not found in catalog"),
        })?;

    // For container mode, download the OCI variant if available.
    if *mode == RuntimeMode::Container {
        if let (Some(container_file), Some(container_sha)) = (
            &catalog_entry.container_file,
            &catalog_entry.container_sha256,
        ) {
            info!(
                name,
                container_file = %container_file,
                "container mode: pulling OCI container variant"
            );

            // Check if already present with matching container SHA (idempotent).
            let oci_path = store.container_image_path(name);
            if let Some(existing) = store.get(name)? {
                if existing.container_sha256.as_deref() == Some(container_sha) && oci_path.exists()
                {
                    info!(
                        name,
                        "OCI image already present with matching SHA, skipping pull"
                    );
                    return Ok(existing);
                }
            }

            // Acquire file lock.
            let lock_path = store.image_dir().join(".lock");
            let lock_file = File::create(&lock_path).map_err(|e| ImageError::ImportFailed {
                reason: format!("failed to create lock file: {e}"),
            })?;
            lock_file
                .lock_exclusive()
                .map_err(|e| ImageError::ImportFailed {
                    reason: format!("failed to acquire lock: {e}"),
                })?;

            let result = pull_oci(store, name, catalog_entry, &catalog.base_url).await;

            let _ = lock_file.unlock();
            drop(lock_file);

            return result;
        }

        info!(
            name,
            "container mode requested but no OCI variant available, falling back to VM image"
        );
    }

    // Default: download the VM raw image.
    pull(store, name, catalog).await
}

/// Download the OCI container variant of an image.
///
/// Unlike [`pull_inner`] which downloads and decompresses the `.raw.gz`, this
/// stores the `.tar.gz` as-is (the container runtime extracts it at create
/// time). The SHA-256 is verified against `container_sha256`.
async fn pull_oci(
    store: &ImageStore,
    name: &str,
    catalog_entry: &ImageMeta,
    base_url: &str,
) -> Result<ImageMeta, ImageError> {
    let container_file = catalog_entry
        .container_file
        .as_deref()
        .expect("pull_oci called without container_file");
    let expected_sha = catalog_entry
        .container_sha256
        .as_deref()
        .expect("pull_oci called without container_sha256");

    let download_url = format!("{}/{}", base_url.trim_end_matches('/'), container_file);
    super::catalog::validate_url(&download_url)?;

    info!(name, url = %download_url, "starting OCI image download");

    let response =
        reqwest::get(&download_url)
            .await
            .map_err(|e| ImageError::CatalogFetchFailed {
                url: download_url.clone(),
                reason: format!("HTTP request failed: {e}"),
            })?;

    if !response.status().is_success() {
        return Err(ImageError::CatalogFetchFailed {
            url: download_url,
            reason: format!("HTTP status {}", response.status()),
        });
    }

    // Stream to a temp file (keep as-is, no decompression).
    let oci_path = store.container_image_path(name);
    let tmp_path = oci_path.with_extension("tar.gz.tmp");
    let mut tmp_file = File::create(&tmp_path).map_err(|e| ImageError::ImportFailed {
        reason: format!("failed to create temp file: {e}"),
    })?;

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| ImageError::CatalogFetchFailed {
            url: download_url.clone(),
            reason: format!("download stream error: {e}"),
        })?;
        tmp_file
            .write_all(&chunk)
            .map_err(|e| ImageError::ImportFailed {
                reason: format!("failed to write to temp file: {e}"),
            })?;
    }

    tmp_file.sync_all().map_err(|e| ImageError::ImportFailed {
        reason: format!("fsync failed: {e}"),
    })?;
    drop(tmp_file);

    // SHA-256 verify.
    let actual_sha = compute_sha256(&tmp_path)?;
    info!(name, sha256 = %actual_sha, "OCI image SHA256 computed");

    if actual_sha != expected_sha {
        let _ = fs::remove_file(&tmp_path);
        return Err(ImageError::ChecksumMismatch {
            expected: expected_sha.to_string(),
            actual: actual_sha,
        });
    }

    info!(name, "OCI image SHA256 verified");

    // Rename to final path: {image_dir}/{name}-oci.tar.gz
    let final_path = store.image_dir().join(format!("{name}-oci.tar.gz"));
    fs::rename(&tmp_path, &final_path).map_err(|e| ImageError::ImportFailed {
        reason: format!("failed to rename temp to final: {e}"),
    })?;

    // Update images.json — store with format "oci" so downstream can tell
    // which variant was pulled.
    let meta = ImageMeta {
        name: name.to_string(),
        arch: catalog_entry.arch.clone(),
        os_family: catalog_entry.os_family.clone(),
        variant: catalog_entry.variant.clone(),
        format: "oci".to_string(),
        compression: Some("gzip".to_string()),
        boot_mode: catalog_entry.boot_mode.clone(),
        sha256: catalog_entry.sha256.clone(),
        size_mb: catalog_entry.size_mb,
        min_disk_mb: catalog_entry.min_disk_mb,
        cloud_init: catalog_entry.cloud_init,
        default_username: catalog_entry.default_username.clone(),
        rootfs_fs: catalog_entry.rootfs_fs.clone(),
        source_kind: "catalog".to_string(),
        file: catalog_entry.file.clone(),
        container_file: catalog_entry.container_file.clone(),
        container_sha256: Some(actual_sha),
        imported_at: Some(chrono_now()),
    };

    let mut images = store.read_metadata()?;
    images.retain(|i| i.name != name);
    images.push(meta.clone());
    store.write_metadata(&images)?;

    info!(name, "OCI image pull complete");
    Ok(meta)
}

async fn pull_inner(
    store: &ImageStore,
    name: &str,
    catalog_entry: &ImageMeta,
    base_url: &str,
) -> Result<ImageMeta, ImageError> {
    // 4. Build download URL from base_url + file name
    let download_url = format!("{}/{}", base_url.trim_end_matches('/'), catalog_entry.file);

    // Validate URL scheme to prevent SSRF (only https:// and http:// allowed)
    super::catalog::validate_url(&download_url)?;

    info!(name, url = %download_url, "starting image download");

    let response =
        reqwest::get(&download_url)
            .await
            .map_err(|e| ImageError::CatalogFetchFailed {
                url: download_url.clone(),
                reason: format!("HTTP request failed: {e}"),
            })?;

    if !response.status().is_success() {
        return Err(ImageError::CatalogFetchFailed {
            url: download_url,
            reason: format!("HTTP status {}", response.status()),
        });
    }

    // Stream to a temp file
    let tmp_path = store.image_dir().join(format!("{name}.raw.tmp"));
    let mut tmp_file = File::create(&tmp_path).map_err(|e| ImageError::ImportFailed {
        reason: format!("failed to create temp file: {e}"),
    })?;

    // Collect the streamed bytes and decompress if gzip
    let is_gzip = catalog_entry
        .compression
        .as_deref()
        .map(|c| c == "gzip" || c == "gz")
        .unwrap_or(false);

    let mut stream = response.bytes_stream();
    let mut compressed_data = Vec::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| ImageError::CatalogFetchFailed {
            url: download_url.clone(),
            reason: format!("download stream error: {e}"),
        })?;

        if is_gzip {
            compressed_data.extend_from_slice(&chunk);
        } else {
            tmp_file
                .write_all(&chunk)
                .map_err(|e| ImageError::ImportFailed {
                    reason: format!("failed to write to temp file: {e}"),
                })?;
        }
    }

    // If gzip, decompress now
    if is_gzip {
        let mut decoder = GzDecoder::new(&compressed_data[..]);
        let mut buf = [0u8; 65536];
        loop {
            let n = decoder
                .read(&mut buf)
                .map_err(|e| ImageError::ImportFailed {
                    reason: format!("gzip decompression failed: {e}"),
                })?;
            if n == 0 {
                break;
            }
            tmp_file
                .write_all(&buf[..n])
                .map_err(|e| ImageError::ImportFailed {
                    reason: format!("failed to write decompressed data: {e}"),
                })?;
        }
    }

    tmp_file.sync_all().map_err(|e| ImageError::ImportFailed {
        reason: format!("fsync failed: {e}"),
    })?;
    drop(tmp_file);

    // 7. SHA256 verify
    let actual_sha = compute_sha256(&tmp_path)?;
    info!(name, sha256 = %actual_sha, "SHA256 computed");

    if actual_sha != catalog_entry.sha256 {
        let _ = fs::remove_file(&tmp_path);
        return Err(ImageError::ChecksumMismatch {
            expected: catalog_entry.sha256.clone(),
            actual: actual_sha,
        });
    }

    info!(name, "SHA256 verified");

    // 8. Rename to final
    let final_path = store.image_path(name);
    fs::rename(&tmp_path, &final_path).map_err(|e| ImageError::ImportFailed {
        reason: format!("failed to rename temp to final: {e}"),
    })?;

    // 9. Update images.json
    let meta = ImageMeta {
        name: name.to_string(),
        arch: catalog_entry.arch.clone(),
        os_family: catalog_entry.os_family.clone(),
        variant: catalog_entry.variant.clone(),
        format: "raw".to_string(),
        compression: None, // already decompressed
        boot_mode: catalog_entry.boot_mode.clone(),
        sha256: actual_sha,
        size_mb: catalog_entry.size_mb,
        min_disk_mb: catalog_entry.min_disk_mb,
        cloud_init: catalog_entry.cloud_init,
        default_username: catalog_entry.default_username.clone(),
        rootfs_fs: catalog_entry.rootfs_fs.clone(),
        source_kind: "catalog".to_string(),
        file: format!("{name}.raw"),
        container_file: catalog_entry.container_file.clone(),
        container_sha256: catalog_entry.container_sha256.clone(),
        imported_at: Some(chrono_now()),
    };

    let mut images = store.read_metadata()?;
    images.retain(|i| i.name != name); // remove old entry if re-pulling
    images.push(meta.clone());
    store.write_metadata(&images)?;

    info!(name, "image pull complete");
    Ok(meta)
}

/// Compute SHA256 hex digest of a file.
pub(crate) fn compute_sha256(path: &std::path::Path) -> Result<String, ImageError> {
    let mut file = File::open(path).map_err(|e| ImageError::ImportFailed {
        reason: format!("failed to open file for hashing: {e}"),
    })?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf).map_err(|e| ImageError::ImportFailed {
            reason: format!("read error during hashing: {e}"),
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn chrono_now() -> String {
    // Simple ISO 8601 timestamp without pulling in chrono
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}Z", dur.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::types::ImageCatalog;
    use std::io::Write;
    use tempfile::TempDir;

    use axum::{extract::Path as AxumPath, routing::get, Router};
    use tokio::net::TcpListener;

    fn sample_catalog_entry(name: &str, sha256: &str, file: &str) -> ImageMeta {
        ImageMeta {
            name: name.to_string(),
            arch: "aarch64".to_string(),
            os_family: "linux".to_string(),
            variant: None,
            format: "raw".to_string(),
            compression: None,
            boot_mode: "uefi".to_string(),
            sha256: sha256.to_string(),
            size_mb: 1,
            min_disk_mb: 1,
            cloud_init: false,
            default_username: None,
            rootfs_fs: None,
            source_kind: "catalog".to_string(),
            file: file.to_string(),
            container_file: None,
            container_sha256: None,
            imported_at: None,
        }
    }

    /// Start a simple HTTP server that serves file content at /images/{name}
    async fn start_test_server(files: Vec<(String, Vec<u8>)>) -> String {
        let files = std::sync::Arc::new(
            files
                .into_iter()
                .collect::<std::collections::HashMap<String, Vec<u8>>>(),
        );

        let app = Router::new().route(
            "/images/{name}",
            get(move |AxumPath(name): AxumPath<String>| {
                let files = files.clone();
                async move {
                    match files.get(&name) {
                        Some(data) => axum::response::Response::builder()
                            .status(200)
                            .body(axum::body::Body::from(data.clone()))
                            .unwrap(),
                        None => axum::response::Response::builder()
                            .status(404)
                            .body(axum::body::Body::empty())
                            .unwrap(),
                    }
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        format!("http://{addr}")
    }

    fn compute_sha_of_bytes(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }

    #[tokio::test]
    async fn pull_ok_checksum_matches() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().to_path_buf());

        let raw_data = b"fake raw image data for test";
        let sha = compute_sha_of_bytes(raw_data);

        let base_url = start_test_server(vec![("test.raw".to_string(), raw_data.to_vec())]).await;

        let catalog = ImageCatalog {
            version: 1,
            base_url: base_url.clone(),
            images: vec![sample_catalog_entry("test-image", &sha, "images/test.raw")],
        };

        let meta = pull(&store, "test-image", &catalog).await.unwrap();
        assert_eq!(meta.name, "test-image");
        assert_eq!(meta.sha256, sha);
        assert!(store.image_path("test-image").exists());
    }

    #[tokio::test]
    async fn pull_checksum_mismatch() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().to_path_buf());

        let raw_data = b"some data";
        let base_url = start_test_server(vec![("bad.raw".to_string(), raw_data.to_vec())]).await;

        let catalog = ImageCatalog {
            version: 1,
            base_url: base_url.clone(),
            images: vec![sample_catalog_entry(
                "bad-image",
                "wrong_sha_value",
                "images/bad.raw",
            )],
        };

        let result = pull(&store, "bad-image", &catalog).await;
        assert!(matches!(result, Err(ImageError::ChecksumMismatch { .. })));
        // Temp file should be cleaned up
        assert!(!tmp.path().join("bad-image.raw.tmp").exists());
        assert!(!store.image_path("bad-image").exists());
    }

    #[tokio::test]
    async fn pull_idempotent_same_sha() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().to_path_buf());

        let sha = "abc123";
        // Pre-populate the store with matching SHA
        let existing = sample_catalog_entry("existing", sha, "existing.raw");
        store
            .write_metadata(std::slice::from_ref(&existing))
            .unwrap();
        // Create the .raw file
        std::fs::write(store.image_path("existing"), b"data").unwrap();

        let catalog = ImageCatalog {
            version: 1,
            base_url: "http://unused".to_string(),
            images: vec![sample_catalog_entry("existing", sha, "existing.raw")],
        };

        // Should return immediately without downloading
        let meta = pull(&store, "existing", &catalog).await.unwrap();
        assert_eq!(meta.sha256, sha);
    }

    #[tokio::test]
    async fn pull_image_not_in_catalog() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().to_path_buf());

        let catalog = ImageCatalog {
            version: 1,
            base_url: "http://unused".to_string(),
            images: vec![],
        };

        let result = pull(&store, "nonexistent", &catalog).await;
        assert!(matches!(result, Err(ImageError::CatalogFetchFailed { .. })));
    }

    #[tokio::test]
    async fn pull_gzip_decompress() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().to_path_buf());

        // Create gzip compressed data
        let raw_data = b"uncompressed raw image content for gzip test";
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(raw_data).unwrap();
        let compressed = encoder.finish().unwrap();

        let sha = compute_sha_of_bytes(raw_data);

        let base_url = start_test_server(vec![("gzip.raw.gz".to_string(), compressed)]).await;

        let mut entry = sample_catalog_entry("gzip-image", &sha, "images/gzip.raw.gz");
        entry.compression = Some("gzip".to_string());

        let catalog = ImageCatalog {
            version: 1,
            base_url: base_url.clone(),
            images: vec![entry],
        };

        let meta = pull(&store, "gzip-image", &catalog).await.unwrap();
        assert_eq!(meta.sha256, sha);

        // Verify the stored file is the decompressed content
        let stored = std::fs::read(store.image_path("gzip-image")).unwrap();
        assert_eq!(stored, raw_data);
    }

    #[tokio::test]
    async fn pull_http_error() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().to_path_buf());

        let base_url = start_test_server(vec![]).await;

        let catalog = ImageCatalog {
            version: 1,
            base_url: base_url.clone(),
            images: vec![sample_catalog_entry(
                "missing",
                "sha",
                "images/nonexistent.raw",
            )],
        };

        let result = pull(&store, "missing", &catalog).await;
        assert!(matches!(result, Err(ImageError::CatalogFetchFailed { .. })));
    }

    #[tokio::test]
    async fn pull_updates_metadata() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().to_path_buf());

        let raw_data = b"metadata test data";
        let sha = compute_sha_of_bytes(raw_data);

        let base_url = start_test_server(vec![("meta.raw".to_string(), raw_data.to_vec())]).await;

        let catalog = ImageCatalog {
            version: 1,
            base_url: base_url.clone(),
            images: vec![sample_catalog_entry("meta-test", &sha, "images/meta.raw")],
        };

        pull(&store, "meta-test", &catalog).await.unwrap();

        let list = store.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "meta-test");
        assert_eq!(list[0].source_kind, "catalog");
    }

    #[tokio::test]
    async fn pull_server_unreachable() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().to_path_buf());

        let catalog = ImageCatalog {
            version: 1,
            base_url: "http://127.0.0.1:1".to_string(), // should fail to connect
            images: vec![sample_catalog_entry(
                "unreachable",
                "sha",
                "http://127.0.0.1:1/images/unreachable.raw",
            )],
        };

        let result = pull(&store, "unreachable", &catalog).await;
        assert!(matches!(result, Err(ImageError::CatalogFetchFailed { .. })));
    }

    #[tokio::test]
    async fn pull_for_runtime_container_downloads_oci() {
        use crate::image::types::RuntimeMode;

        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().to_path_buf());

        let oci_data = b"fake OCI tar.gz content";
        let oci_sha = compute_sha_of_bytes(oci_data);

        let base_url =
            start_test_server(vec![("alpine-oci.tar.gz".to_string(), oci_data.to_vec())]).await;

        let mut entry = sample_catalog_entry("alpine", "vm_sha", "images/alpine.raw");
        entry.container_file = Some("images/alpine-oci.tar.gz".to_string());
        entry.container_sha256 = Some(oci_sha.clone());

        let catalog = ImageCatalog {
            version: 1,
            base_url: base_url.clone(),
            images: vec![entry],
        };

        let meta = pull_for_runtime(&store, "alpine", &catalog, &RuntimeMode::Container)
            .await
            .unwrap();

        assert_eq!(meta.format, "oci");
        assert_eq!(meta.container_sha256.as_deref(), Some(oci_sha.as_str()));

        // The OCI tar.gz should exist on disk.
        let oci_path = tmp.path().join("alpine-oci.tar.gz");
        assert!(oci_path.exists());
        let stored = std::fs::read(&oci_path).unwrap();
        assert_eq!(stored, oci_data);
    }

    #[tokio::test]
    async fn pull_for_runtime_container_fallback_to_vm() {
        use crate::image::types::RuntimeMode;

        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().to_path_buf());

        let raw_data = b"fake raw image";
        let sha = compute_sha_of_bytes(raw_data);

        let base_url = start_test_server(vec![("alpine.raw".to_string(), raw_data.to_vec())]).await;

        // No container_file — should fall back to VM variant.
        let entry = sample_catalog_entry("alpine", &sha, "images/alpine.raw");

        let catalog = ImageCatalog {
            version: 1,
            base_url: base_url.clone(),
            images: vec![entry],
        };

        let meta = pull_for_runtime(&store, "alpine", &catalog, &RuntimeMode::Container)
            .await
            .unwrap();

        assert_eq!(meta.format, "raw");
        assert!(store.image_path("alpine").exists());
    }

    #[tokio::test]
    async fn pull_for_runtime_vm_mode_uses_raw() {
        use crate::image::types::RuntimeMode;

        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().to_path_buf());

        let raw_data = b"fake raw image for vm mode";
        let sha = compute_sha_of_bytes(raw_data);

        let base_url = start_test_server(vec![("test.raw".to_string(), raw_data.to_vec())]).await;

        let mut entry = sample_catalog_entry("test-image", &sha, "images/test.raw");
        entry.container_file = Some("images/test-oci.tar.gz".to_string());
        entry.container_sha256 = Some("oci_sha".to_string());

        let catalog = ImageCatalog {
            version: 1,
            base_url: base_url.clone(),
            images: vec![entry],
        };

        // VM mode should download .raw, not OCI.
        let meta = pull_for_runtime(&store, "test-image", &catalog, &RuntimeMode::Vm)
            .await
            .unwrap();

        assert_eq!(meta.format, "raw");
        assert!(store.image_path("test-image").exists());
    }
}
