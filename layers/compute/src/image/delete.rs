use std::collections::HashMap;
use std::fs::{self, File};

use fs2::FileExt;
use tracing::info;

use super::error::ImageError;
use super::store::ImageStore;

/// Delete an image from the store.
///
/// Refuses to delete if any VMs reference the image (refcount protection).
/// Acquires a file lock, removes the `.raw` file, and updates `images.json`.
pub fn delete(
    store: &ImageStore,
    name: &str,
    vm_refs: &HashMap<String, u32>,
) -> Result<(), ImageError> {
    // 1. Check image exists
    if !store.exists(name) {
        return Err(ImageError::ImageNotFound {
            name: name.to_string(),
        });
    }

    // 2. Check refcount
    if let Some(&count) = vm_refs.get(name) {
        if count > 0 {
            return Err(ImageError::ImageInUse {
                name: name.to_string(),
                vm_count: count,
            });
        }
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

    let result = delete_inner(store, name);

    let _ = lock_file.unlock();
    drop(lock_file);

    result
}

fn delete_inner(store: &ImageStore, name: &str) -> Result<(), ImageError> {
    // 4. Delete .raw file
    let raw_path = store.image_path(name);
    if raw_path.exists() {
        fs::remove_file(&raw_path).map_err(|e| ImageError::ImportFailed {
            reason: format!("failed to delete image file: {e}"),
        })?;
    }

    // 5. Remove from images.json
    let mut images = store.read_metadata()?;
    images.retain(|i| i.name != name);
    store.write_metadata(&images)?;

    info!(name, "image deleted");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::store::ImageStore;
    use crate::image::types::ImageMeta;
    use tempfile::TempDir;

    fn sample_meta(name: &str) -> ImageMeta {
        ImageMeta {
            name: name.to_string(),
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
            default_username: None,
            rootfs_fs: None,
            source_kind: "catalog".to_string(),
            file: format!("{name}.raw"),
            container_file: None,
            container_sha256: None,
            imported_at: None,
        }
    }

    #[test]
    fn delete_ok() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().to_path_buf());

        store.write_metadata(&[sample_meta("to-delete")]).unwrap();
        fs::write(store.image_path("to-delete"), b"data").unwrap();

        let refs = HashMap::new();
        delete(&store, "to-delete", &refs).unwrap();

        assert!(!store.image_path("to-delete").exists());
        assert!(!store.exists("to-delete"));
    }

    #[test]
    fn delete_in_use() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().to_path_buf());

        store.write_metadata(&[sample_meta("busy")]).unwrap();
        fs::write(store.image_path("busy"), b"data").unwrap();

        let mut refs = HashMap::new();
        refs.insert("busy".to_string(), 2);

        let result = delete(&store, "busy", &refs);
        match result {
            Err(ImageError::ImageInUse { name, vm_count }) => {
                assert_eq!(name, "busy");
                assert_eq!(vm_count, 2);
            }
            other => panic!("expected ImageInUse, got {other:?}"),
        }

        // Image should still exist
        assert!(store.image_path("busy").exists());
    }

    #[test]
    fn delete_not_found() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().to_path_buf());
        store.write_metadata(&[]).unwrap();

        let refs = HashMap::new();
        let result = delete(&store, "nonexistent", &refs);
        assert!(matches!(result, Err(ImageError::ImageNotFound { .. })));
    }

    #[test]
    fn delete_removes_from_list() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().to_path_buf());

        store
            .write_metadata(&[sample_meta("keep"), sample_meta("remove")])
            .unwrap();
        fs::write(store.image_path("keep"), b"data").unwrap();
        fs::write(store.image_path("remove"), b"data").unwrap();

        let refs = HashMap::new();
        delete(&store, "remove", &refs).unwrap();

        let list = store.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "keep");
    }

    #[test]
    fn delete_zero_refcount_allowed() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().to_path_buf());

        store.write_metadata(&[sample_meta("zero-refs")]).unwrap();
        fs::write(store.image_path("zero-refs"), b"data").unwrap();

        let mut refs = HashMap::new();
        refs.insert("zero-refs".to_string(), 0);

        delete(&store, "zero-refs", &refs).unwrap();
        assert!(!store.exists("zero-refs"));
    }
}
