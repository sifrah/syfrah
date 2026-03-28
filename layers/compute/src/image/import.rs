use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use fs2::FileExt;
use tracing::info;

use super::error::ImageError;
use super::pull::compute_sha256;
use super::store::ImageStore;
use super::types::ImageMeta;

/// QCOW2 magic bytes: `QFI\xfb`
const QCOW2_MAGIC: [u8; 4] = [0x51, 0x46, 0x49, 0xfb];

/// Import a local raw disk image into the store.
///
/// Validates the file exists, rejects qcow2 format, copies the file,
/// computes SHA256, and updates metadata atomically.
pub fn import(
    store: &ImageStore,
    path: &Path,
    name: &str,
    arch: &str,
) -> Result<ImageMeta, ImageError> {
    // 1. Validate file exists
    if !path.exists() {
        return Err(ImageError::ImportFailed {
            reason: format!("file not found: {}", path.display()),
        });
    }

    // 2. Validate format: reject qcow2
    {
        let mut f = File::open(path).map_err(|e| ImageError::ImportFailed {
            reason: format!("cannot open file: {e}"),
        })?;
        let mut magic = [0u8; 4];
        if f.read_exact(&mut magic).is_ok() && magic == QCOW2_MAGIC {
            return Err(ImageError::InvalidImageFormat {
                detail: "qcow2 images are not supported, convert to raw first".to_string(),
            });
        }
    }

    // 3. Check name doesn't already exist
    if store.exists(name) {
        return Err(ImageError::ImageAlreadyExists {
            name: name.to_string(),
        });
    }

    // 4. Acquire file lock
    let lock_path = store.image_dir().join(".lock");
    let lock_file = File::create(&lock_path).map_err(|e| ImageError::ImportFailed {
        reason: format!("failed to create lock file: {e}"),
    })?;
    lock_file
        .lock_exclusive()
        .map_err(|e| ImageError::ImportFailed {
            reason: format!("failed to acquire lock: {e}"),
        })?;

    let result = import_inner(store, path, name, arch);

    let _ = lock_file.unlock();
    drop(lock_file);

    result
}

fn import_inner(
    store: &ImageStore,
    path: &Path,
    name: &str,
    arch: &str,
) -> Result<ImageMeta, ImageError> {
    // Read existing metadata BEFORE copying the file (so scan_raw_files
    // doesn't pick up the new .raw and create a duplicate entry).
    let mut images = store.read_metadata()?;

    // 5. Copy file
    let dest = store.image_path(name);
    fs::copy(path, &dest).map_err(|e| ImageError::ImportFailed {
        reason: format!("failed to copy image: {e}"),
    })?;

    // 6. Compute SHA256
    let sha256 = compute_sha256(&dest)?;

    // 7. Build metadata
    let size_mb = fs::metadata(&dest)
        .map(|m| m.len() / (1024 * 1024))
        .unwrap_or(0);

    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();

    let meta = ImageMeta {
        name: name.to_string(),
        arch: arch.to_string(),
        os_family: "linux".to_string(),
        variant: None,
        format: "raw".to_string(),
        compression: None,
        boot_mode: "uefi".to_string(),
        sha256,
        size_mb,
        min_disk_mb: size_mb,
        cloud_init: false,
        default_username: None,
        rootfs_fs: None,
        source_kind: "custom".to_string(),
        file: format!("{name}.raw"),
        container_file: None,
        container_sha256: None,
        imported_at: Some(format!("{}Z", dur.as_secs())),
    };

    // 8. Update images.json (using metadata read before copy to avoid scan
    //    duplicates)
    images.push(meta.clone());
    store.write_metadata(&images)?;

    info!(name, "image imported successfully");
    Ok(meta)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn import_ok() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().join("store"));
        fs::create_dir_all(store.image_dir()).unwrap();

        // Create a source file
        let src = tmp.path().join("source.raw");
        fs::write(&src, b"raw disk image content").unwrap();

        let meta = import(&store, &src, "my-image", "x86_64").unwrap();
        assert_eq!(meta.name, "my-image");
        assert_eq!(meta.arch, "x86_64");
        assert_eq!(meta.source_kind, "custom");
        assert!(!meta.cloud_init);
        assert!(meta.imported_at.is_some());
        assert!(store.image_path("my-image").exists());
    }

    #[test]
    fn import_duplicate_name() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().join("store"));
        fs::create_dir_all(store.image_dir()).unwrap();

        let src = tmp.path().join("source.raw");
        fs::write(&src, b"data").unwrap();

        import(&store, &src, "dupe", "x86_64").unwrap();
        let result = import(&store, &src, "dupe", "x86_64");
        assert!(matches!(result, Err(ImageError::ImageAlreadyExists { .. })));
    }

    #[test]
    fn import_file_missing() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().join("store"));
        fs::create_dir_all(store.image_dir()).unwrap();

        let result = import(&store, Path::new("/nonexistent/file.raw"), "test", "x86_64");
        assert!(matches!(result, Err(ImageError::ImportFailed { .. })));
    }

    #[test]
    fn import_qcow2_rejected() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().join("store"));
        fs::create_dir_all(store.image_dir()).unwrap();

        // Create a file with qcow2 magic bytes
        let src = tmp.path().join("disk.qcow2");
        let mut data = QCOW2_MAGIC.to_vec();
        data.extend_from_slice(b"rest of qcow2 header");
        fs::write(&src, &data).unwrap();

        let result = import(&store, &src, "qcow-image", "x86_64");
        assert!(matches!(result, Err(ImageError::InvalidImageFormat { .. })));
    }

    #[test]
    fn import_metadata_has_correct_fields() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().join("store"));
        fs::create_dir_all(store.image_dir()).unwrap();

        let src = tmp.path().join("disk.raw");
        fs::write(&src, b"test content for sha verification").unwrap();

        let meta = import(&store, &src, "field-check", "aarch64").unwrap();
        assert_eq!(meta.format, "raw");
        assert_eq!(meta.source_kind, "custom");
        assert!(!meta.sha256.is_empty());
        assert_eq!(meta.arch, "aarch64");

        // Verify metadata is persisted
        let stored = store.get("field-check").unwrap().unwrap();
        assert_eq!(stored.sha256, meta.sha256);
    }

    #[test]
    fn import_sha256_is_correct() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().join("store"));
        fs::create_dir_all(store.image_dir()).unwrap();

        let content = b"deterministic content for sha test";
        let src = tmp.path().join("sha-test.raw");
        fs::write(&src, content).unwrap();

        let meta = import(&store, &src, "sha-test", "x86_64").unwrap();

        // Compute expected SHA
        use sha2::Digest;
        let mut hasher = sha2::Sha256::new();
        hasher.update(content);
        let expected = format!("{:x}", hasher.finalize());

        assert_eq!(meta.sha256, expected);
    }
}
