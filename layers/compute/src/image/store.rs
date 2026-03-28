use std::fs;
use std::io::Write;
use std::path::PathBuf;

use tracing::warn;

use super::error::ImageError;
use super::types::ImageMeta;

/// Local image store backed by a directory of `.raw` files and an `images.json`
/// metadata index.
///
/// All metadata mutations go through [`write_metadata`](Self::write_metadata),
/// which uses atomic write (tmp + fsync + rename) to prevent corruption.
pub struct ImageStore {
    image_dir: PathBuf,
}

impl ImageStore {
    /// Create a new `ImageStore` rooted at `image_dir`.
    pub fn new(image_dir: PathBuf) -> Self {
        Self { image_dir }
    }

    /// Return the root directory of this store.
    pub fn image_dir(&self) -> &PathBuf {
        &self.image_dir
    }

    /// List all images known to the store.
    pub fn list(&self) -> Result<Vec<ImageMeta>, ImageError> {
        self.read_metadata()
    }

    /// Look up a single image by name.
    pub fn get(&self, name: &str) -> Result<Option<ImageMeta>, ImageError> {
        let images = self.read_metadata()?;
        Ok(images.into_iter().find(|i| i.name == name))
    }

    /// Check whether an image with the given name exists in metadata.
    pub fn exists(&self, name: &str) -> bool {
        self.read_metadata()
            .map(|imgs| imgs.iter().any(|i| i.name == name))
            .unwrap_or(false)
    }

    /// Read and parse `images.json`. Returns an empty vec if the file is
    /// missing or corrupt (logs a warning on corruption).
    pub fn read_metadata(&self) -> Result<Vec<ImageMeta>, ImageError> {
        let path = self.metadata_path();
        match fs::read_to_string(&path) {
            Ok(content) => match serde_json::from_str::<Vec<ImageMeta>>(&content) {
                Ok(images) => Ok(images),
                Err(e) => {
                    warn!(
                        path = %path.display(),
                        error = %e,
                        "corrupt images.json, returning empty list"
                    );
                    Ok(vec![])
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Scan for .raw files and build basic metadata
                self.scan_raw_files()
            }
            Err(e) => Err(ImageError::ImportFailed {
                reason: format!("failed to read images.json: {e}"),
            }),
        }
    }

    /// Atomically write the metadata list to `images.json`.
    ///
    /// Strategy: write to a `.tmp` file, fsync, then rename over the real file.
    pub fn write_metadata(&self, images: &[ImageMeta]) -> Result<(), ImageError> {
        let path = self.metadata_path();
        let tmp_path = path.with_extension("json.tmp");

        let json = serde_json::to_string_pretty(images).map_err(|e| ImageError::ImportFailed {
            reason: format!("failed to serialize metadata: {e}"),
        })?;

        let mut file = fs::File::create(&tmp_path).map_err(|e| ImageError::ImportFailed {
            reason: format!("failed to create tmp metadata file: {e}"),
        })?;

        file.write_all(json.as_bytes())
            .map_err(|e| ImageError::ImportFailed {
                reason: format!("failed to write tmp metadata file: {e}"),
            })?;

        file.sync_all().map_err(|e| ImageError::ImportFailed {
            reason: format!("failed to fsync tmp metadata file: {e}"),
        })?;

        fs::rename(&tmp_path, &path).map_err(|e| ImageError::ImportFailed {
            reason: format!("failed to rename tmp metadata file: {e}"),
        })?;

        Ok(())
    }

    /// Return the expected path of a `.raw` image file.
    pub fn image_path(&self, name: &str) -> PathBuf {
        self.image_dir.join(format!("{name}.raw"))
    }

    /// Return the expected path of an extracted OCI image directory.
    pub fn container_image_path(&self, name: &str) -> PathBuf {
        self.image_dir.join(format!("{name}-oci"))
    }

    // ---- private helpers ----------------------------------------------------

    fn metadata_path(&self) -> PathBuf {
        self.image_dir.join("images.json")
    }

    /// Scan the image directory for `.raw` files and build a minimal metadata
    /// list. This is the fallback when `images.json` doesn't exist.
    fn scan_raw_files(&self) -> Result<Vec<ImageMeta>, ImageError> {
        let entries = match fs::read_dir(&self.image_dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => {
                return Err(ImageError::ImportFailed {
                    reason: format!("failed to scan image dir: {e}"),
                })
            }
        };

        let mut images = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("raw") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    let size_mb = fs::metadata(&path)
                        .map(|m| m.len() / (1024 * 1024))
                        .unwrap_or(0);
                    images.push(ImageMeta {
                        name: stem.to_string(),
                        arch: "unknown".to_string(),
                        os_family: "unknown".to_string(),
                        variant: None,
                        format: "raw".to_string(),
                        compression: None,
                        boot_mode: "unknown".to_string(),
                        sha256: String::new(),
                        size_mb,
                        min_disk_mb: size_mb,
                        cloud_init: false,
                        default_username: None,
                        rootfs_fs: None,
                        source_kind: "scan".to_string(),
                        file: format!("{stem}.raw"),
                        container_file: None,
                        container_sha256: None,
                        imported_at: None,
                    });
                }
            }
        }
        Ok(images)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            default_username: Some("ubuntu".to_string()),
            rootfs_fs: Some("ext4".to_string()),
            source_kind: "catalog".to_string(),
            file: format!("{name}.raw"),
            container_file: None,
            container_sha256: None,
            imported_at: None,
        }
    }

    #[test]
    fn empty_dir_returns_empty_list() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().to_path_buf());
        let list = store.list().unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn two_images_listed() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().to_path_buf());
        let images = vec![sample_meta("ubuntu-24.04"), sample_meta("alpine-3.20")];
        store.write_metadata(&images).unwrap();

        let list = store.list().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "ubuntu-24.04");
        assert_eq!(list[1].name, "alpine-3.20");
    }

    #[test]
    fn get_existing_image() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().to_path_buf());
        store
            .write_metadata(&[sample_meta("ubuntu-24.04")])
            .unwrap();

        let result = store.get("ubuntu-24.04").unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "ubuntu-24.04");
    }

    #[test]
    fn get_unknown_image() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().to_path_buf());
        store
            .write_metadata(&[sample_meta("ubuntu-24.04")])
            .unwrap();

        let result = store.get("nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn exists_true() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().to_path_buf());
        store
            .write_metadata(&[sample_meta("ubuntu-24.04")])
            .unwrap();

        assert!(store.exists("ubuntu-24.04"));
    }

    #[test]
    fn exists_false() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().to_path_buf());
        store
            .write_metadata(&[sample_meta("ubuntu-24.04")])
            .unwrap();

        assert!(!store.exists("nonexistent"));
    }

    #[test]
    fn corrupt_json_returns_empty_list() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().to_path_buf());

        // Write garbage to images.json
        fs::write(tmp.path().join("images.json"), "not valid json {{{").unwrap();

        let list = store.read_metadata().unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn atomic_write_verified() {
        let tmp = TempDir::new().unwrap();
        let store = ImageStore::new(tmp.path().to_path_buf());
        let images = vec![sample_meta("test-image")];
        store.write_metadata(&images).unwrap();

        // Read back the raw file and verify it's valid JSON
        let content = fs::read_to_string(tmp.path().join("images.json")).unwrap();
        let parsed: Vec<ImageMeta> = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "test-image");

        // Verify no tmp file left behind
        assert!(!tmp.path().join("images.json.tmp").exists());
    }

    #[test]
    fn image_path_format() {
        let store = ImageStore::new(PathBuf::from("/opt/syfrah/images"));
        assert_eq!(
            store.image_path("ubuntu-24.04"),
            PathBuf::from("/opt/syfrah/images/ubuntu-24.04.raw")
        );
    }

    #[test]
    fn missing_json_scans_raw_files() {
        let tmp = TempDir::new().unwrap();
        // Create some .raw files but no images.json
        fs::write(tmp.path().join("ubuntu.raw"), "fake image data").unwrap();
        fs::write(tmp.path().join("alpine.raw"), "fake image data").unwrap();
        // Non-raw file should be ignored
        fs::write(tmp.path().join("notes.txt"), "not an image").unwrap();

        let store = ImageStore::new(tmp.path().to_path_buf());
        let list = store.list().unwrap();
        assert_eq!(list.len(), 2);

        let names: Vec<&str> = list.iter().map(|i| i.name.as_str()).collect();
        assert!(names.contains(&"ubuntu"));
        assert!(names.contains(&"alpine"));
    }
}
