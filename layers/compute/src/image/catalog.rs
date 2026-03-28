use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::SystemTime;

use tracing::info;

use super::error::ImageError;
use super::types::{ImageCatalog, PullPolicy};

/// Validate that a URL uses an allowed scheme (https:// or http:// only).
///
/// Rejects file://, ftp://, and any other scheme to prevent SSRF attacks
/// where an operator might point at internal metadata endpoints.
pub fn validate_url(url: &str) -> Result<(), ImageError> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err(ImageError::CatalogFetchFailed {
            url: url.to_string(),
            reason: "only https:// and http:// URLs are allowed".to_string(),
        });
    }
    Ok(())
}

/// Fetch the image catalog from a remote URL with caching.
///
/// Behavior depends on `policy`:
/// - **IfNotPresent**: use cache if it exists and is < 1 hour old, else fetch
/// - **Always**: always fetch from remote
/// - **Never**: cache only; `CatalogUnavailable` if no cache
pub async fn fetch_catalog(
    url: &str,
    cache_path: &Path,
    policy: PullPolicy,
) -> Result<ImageCatalog, ImageError> {
    if url.is_empty() && !matches!(policy, PullPolicy::Never) {
        return Err(ImageError::CatalogFetchFailed {
            url: "(not configured)".to_string(),
            reason: "No image catalog URL configured \u{2014} set [compute.images] catalog_url in ~/.syfrah/config.toml or use the default".to_string(),
        });
    }
    if !matches!(policy, PullPolicy::Never) {
        validate_url(url)?;
    }
    match policy {
        PullPolicy::Never => read_cache(cache_path),
        PullPolicy::Always => fetch_and_cache(url, cache_path).await,
        PullPolicy::IfNotPresent => {
            if is_cache_fresh(cache_path) {
                info!("using cached catalog");
                read_cache(cache_path)
            } else {
                fetch_and_cache(url, cache_path).await
            }
        }
    }
}

/// Check if the cache file exists and is less than 1 hour old.
fn is_cache_fresh(cache_path: &Path) -> bool {
    let meta = match fs::metadata(cache_path) {
        Ok(m) => m,
        Err(_) => return false,
    };
    let modified = match meta.modified() {
        Ok(t) => t,
        Err(_) => return false,
    };
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or_default();
    age.as_secs() < 3600
}

/// Read and parse the cached catalog file.
fn read_cache(cache_path: &Path) -> Result<ImageCatalog, ImageError> {
    let content = fs::read_to_string(cache_path).map_err(|_| ImageError::CatalogUnavailable)?;
    serde_json::from_str(&content).map_err(|e| ImageError::CatalogFetchFailed {
        url: cache_path.display().to_string(),
        reason: format!("failed to parse cached catalog: {e}"),
    })
}

/// Fetch the catalog from the remote URL and write it to the cache atomically.
async fn fetch_and_cache(url: &str, cache_path: &Path) -> Result<ImageCatalog, ImageError> {
    info!(url, "fetching catalog");

    let response = reqwest::get(url)
        .await
        .map_err(|e| ImageError::CatalogFetchFailed {
            url: url.to_string(),
            reason: format!("HTTP request failed: {e}"),
        })?;

    if !response.status().is_success() {
        return Err(ImageError::CatalogFetchFailed {
            url: url.to_string(),
            reason: format!("HTTP status {}", response.status()),
        });
    }

    let body = response
        .text()
        .await
        .map_err(|e| ImageError::CatalogFetchFailed {
            url: url.to_string(),
            reason: format!("failed to read response body: {e}"),
        })?;

    let catalog: ImageCatalog =
        serde_json::from_str(&body).map_err(|e| ImageError::CatalogFetchFailed {
            url: url.to_string(),
            reason: format!("failed to parse catalog JSON: {e}"),
        })?;

    // Atomic write to cache
    if let Some(parent) = cache_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp_path = cache_path.with_extension("json.tmp");
    let mut file = fs::File::create(&tmp_path).map_err(|e| ImageError::CatalogFetchFailed {
        url: url.to_string(),
        reason: format!("failed to create cache tmp file: {e}"),
    })?;
    file.write_all(body.as_bytes())
        .map_err(|e| ImageError::CatalogFetchFailed {
            url: url.to_string(),
            reason: format!("failed to write cache: {e}"),
        })?;
    file.sync_all()
        .map_err(|e| ImageError::CatalogFetchFailed {
            url: url.to_string(),
            reason: format!("fsync failed: {e}"),
        })?;
    fs::rename(&tmp_path, cache_path).map_err(|e| ImageError::CatalogFetchFailed {
        url: url.to_string(),
        reason: format!("rename failed: {e}"),
    })?;

    info!(url, "catalog fetched and cached");
    Ok(catalog)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::types::ImageMeta;
    use axum::{routing::get, Router};
    use tempfile::TempDir;
    use tokio::net::TcpListener;

    fn sample_catalog() -> ImageCatalog {
        ImageCatalog {
            version: 1,
            base_url: "https://images.syfrah.dev/v1".to_string(),
            images: vec![ImageMeta {
                name: "ubuntu-24.04".to_string(),
                arch: "aarch64".to_string(),
                os_family: "linux".to_string(),
                variant: None,
                format: "raw".to_string(),
                compression: None,
                boot_mode: "uefi".to_string(),
                sha256: "abc123".to_string(),
                size_mb: 1024,
                min_disk_mb: 2048,
                cloud_init: true,
                default_username: Some("ubuntu".to_string()),
                rootfs_fs: Some("ext4".to_string()),
                source_kind: "catalog".to_string(),
                file: "ubuntu-24.04.raw".to_string(),
                container_file: None,
                container_sha256: None,
                imported_at: None,
            }],
        }
    }

    async fn start_catalog_server(catalog: &ImageCatalog) -> String {
        let json = serde_json::to_string(catalog).unwrap();
        let app = Router::new().route(
            "/catalog.json",
            get(move || {
                let json = json.clone();
                async move { json }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        format!("http://{addr}")
    }

    async fn start_error_server() -> String {
        let app = Router::new().route(
            "/catalog.json",
            get(|| async { (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "error") }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        format!("http://{addr}")
    }

    #[tokio::test]
    async fn cache_hit_fresh() {
        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("catalog.json");
        let catalog = sample_catalog();

        // Write fresh cache
        fs::write(&cache_path, serde_json::to_string(&catalog).unwrap()).unwrap();

        let result = fetch_catalog("http://unused", &cache_path, PullPolicy::IfNotPresent).await;
        let fetched = result.unwrap();
        assert_eq!(fetched.images.len(), 1);
        assert_eq!(fetched.images[0].name, "ubuntu-24.04");
    }

    #[tokio::test]
    async fn cache_miss_fetches() {
        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("catalog.json");
        let catalog = sample_catalog();

        let base_url = start_catalog_server(&catalog).await;
        let url = format!("{base_url}/catalog.json");

        // No cache file exists
        let result = fetch_catalog(&url, &cache_path, PullPolicy::IfNotPresent).await;
        let fetched = result.unwrap();
        assert_eq!(fetched.images.len(), 1);

        // Cache should now exist
        assert!(cache_path.exists());
    }

    #[tokio::test]
    async fn always_fetches() {
        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("catalog.json");
        let catalog = sample_catalog();

        // Write old cache with different content
        let old = ImageCatalog {
            version: 1,
            base_url: "old".to_string(),
            images: vec![],
        };
        fs::write(&cache_path, serde_json::to_string(&old).unwrap()).unwrap();

        let base_url = start_catalog_server(&catalog).await;
        let url = format!("{base_url}/catalog.json");

        let result = fetch_catalog(&url, &cache_path, PullPolicy::Always).await;
        let fetched = result.unwrap();
        // Should have fetched new catalog, not old cache
        assert_eq!(fetched.images.len(), 1);
        assert_eq!(fetched.base_url, "https://images.syfrah.dev/v1");
    }

    #[tokio::test]
    async fn never_with_cache() {
        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("catalog.json");
        let catalog = sample_catalog();

        fs::write(&cache_path, serde_json::to_string(&catalog).unwrap()).unwrap();

        let result = fetch_catalog("http://unused", &cache_path, PullPolicy::Never).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn never_no_cache() {
        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("catalog.json");

        let result = fetch_catalog("http://unused", &cache_path, PullPolicy::Never).await;
        assert!(matches!(result, Err(ImageError::CatalogUnavailable)));
    }

    #[tokio::test]
    async fn fetch_http_error() {
        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("catalog.json");

        let base_url = start_error_server().await;
        let url = format!("{base_url}/catalog.json");

        let result = fetch_catalog(&url, &cache_path, PullPolicy::Always).await;
        assert!(matches!(result, Err(ImageError::CatalogFetchFailed { .. })));
    }

    #[tokio::test]
    async fn malformed_json_error() {
        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("catalog.json");

        // Write malformed JSON to cache
        fs::write(&cache_path, "not valid json {{").unwrap();

        let result = fetch_catalog("http://unused", &cache_path, PullPolicy::Never).await;
        assert!(matches!(result, Err(ImageError::CatalogFetchFailed { .. })));
    }

    #[tokio::test]
    async fn stale_cache_refetches() {
        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("catalog.json");
        let catalog = sample_catalog();

        // Write cache and backdate its mtime to 2 hours ago
        fs::write(&cache_path, serde_json::to_string(&catalog).unwrap()).unwrap();
        let two_hours_ago = filetime::FileTime::from_unix_time(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64
                - 7200,
            0,
        );
        filetime::set_file_mtime(&cache_path, two_hours_ago).unwrap();

        let base_url = start_catalog_server(&catalog).await;
        let url = format!("{base_url}/catalog.json");

        let result = fetch_catalog(&url, &cache_path, PullPolicy::IfNotPresent).await;
        assert!(result.is_ok());
    }

    #[test]
    fn validate_url_accepts_https() {
        assert!(validate_url("https://images.syfrah.dev/catalog.json").is_ok());
    }

    #[test]
    fn validate_url_accepts_http() {
        assert!(validate_url("http://localhost:8080/catalog.json").is_ok());
    }

    #[test]
    fn validate_url_rejects_file() {
        let result = validate_url("file:///etc/passwd");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("only https:// and http://"));
    }

    #[test]
    fn validate_url_rejects_ftp() {
        assert!(validate_url("ftp://evil.example.com/data").is_err());
    }

    #[test]
    fn validate_url_rejects_empty() {
        assert!(validate_url("").is_err());
    }

    #[tokio::test]
    async fn fetch_catalog_rejects_file_url() {
        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("catalog.json");

        let result = fetch_catalog("file:///etc/passwd", &cache_path, PullPolicy::Always).await;
        assert!(matches!(result, Err(ImageError::CatalogFetchFailed { .. })));
        let err = result.unwrap_err().to_string();
        assert!(err.contains("only https:// and http://"));
    }

    #[tokio::test]
    async fn fetch_catalog_never_skips_url_validation() {
        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("catalog.json");
        let catalog = sample_catalog();
        fs::write(&cache_path, serde_json::to_string(&catalog).unwrap()).unwrap();

        // PullPolicy::Never should still work even with a bad URL since it
        // only reads from cache and skips URL validation
        let result = fetch_catalog("file:///etc/passwd", &cache_path, PullPolicy::Never).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn fetch_catalog_empty_url_shows_not_configured() {
        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("catalog.json");

        let result = fetch_catalog("", &cache_path, PullPolicy::Always).await;
        match result {
            Err(ImageError::CatalogFetchFailed { url, .. }) => {
                assert_eq!(url, "(not configured)");
            }
            other => panic!("expected CatalogFetchFailed, got {other:?}"),
        }
    }
}
