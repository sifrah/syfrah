use std::fs::{self, File};
use std::io::{Read, Write};

use flate2::read::GzDecoder;
use fs2::FileExt;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tracing::info;

use super::error::ImageError;
use super::store::ImageStore;
use super::types::{ImageCatalog, ImageMeta};

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

    let result = pull_inner(store, name, catalog_entry).await;

    // Release lock (drop does it, but be explicit)
    let _ = lock_file.unlock();
    drop(lock_file);

    result
}

async fn pull_inner(
    store: &ImageStore,
    name: &str,
    catalog_entry: &ImageMeta,
) -> Result<ImageMeta, ImageError> {
    // 4. Download streaming — use file field as URL directly (catalog populates
    //    full URLs or the caller passes them in via the file field)
    let download_url = catalog_entry.file.clone();

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
            images: vec![sample_catalog_entry(
                "test-image",
                &sha,
                &format!("{base_url}/images/test.raw"),
            )],
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
                &format!("{base_url}/images/bad.raw"),
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

        let mut entry = sample_catalog_entry(
            "gzip-image",
            &sha,
            &format!("{base_url}/images/gzip.raw.gz"),
        );
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
                &format!("{base_url}/images/nonexistent.raw"),
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
            images: vec![sample_catalog_entry(
                "meta-test",
                &sha,
                &format!("{base_url}/images/meta.raw"),
            )],
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
}
