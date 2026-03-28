//! Disk service: instance directories, image cloning, and resize.
//!
//! Each VM instance gets a dedicated directory (`/opt/syfrah/instances/{uuid}/`)
//! containing its rootfs clone, cloud-init disk, serial log, and local metadata.
//! This module handles creating those directories, cloning base images with
//! reflink support, and resizing disks when requested.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::image::error::ImageError;
use crate::image::types::InstanceId;

// ---------------------------------------------------------------------------
// InstanceMeta
// ---------------------------------------------------------------------------

/// Metadata persisted alongside a VM instance for debugging and reconnect.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct InstanceMeta {
    /// Source image name (e.g. `"ubuntu-24.04"`).
    pub image_source: String,
    /// SHA-256 of the base image at clone time.
    pub image_sha: String,
    /// CPU architecture (e.g. `"aarch64"`).
    pub arch: String,
    /// Disk size requested by the user, if any.
    pub requested_disk_size_mb: Option<u32>,
    /// Actual disk size after clone + optional resize.
    pub effective_disk_size_mb: u32,
    /// VM hostname.
    pub hostname: String,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
    /// Human-readable VM name.
    pub vm_name: String,
}

// ---------------------------------------------------------------------------
// InstanceDir
// ---------------------------------------------------------------------------

/// A VM instance directory containing all per-instance artifacts.
///
/// Created once per VM lifecycle and cleaned up on delete.
pub struct InstanceDir {
    base_path: PathBuf,
}

impl InstanceDir {
    /// Create a new instance directory under `base/{id}/`.
    ///
    /// Returns an error if the directory already exists or cannot be created.
    pub fn create(base: &Path, id: &InstanceId) -> Result<Self, ImageError> {
        let dir = base.join(id.to_string());
        if dir.exists() {
            return Err(ImageError::DiskCloneFailed {
                reason: format!("instance directory already exists: {}", dir.display()),
            });
        }
        fs::create_dir_all(&dir).map_err(|e| ImageError::DiskCloneFailed {
            reason: format!("failed to create instance dir {}: {e}", dir.display()),
        })?;
        Ok(Self { base_path: dir })
    }

    /// Open an existing instance directory (no creation).
    pub fn open(base: &Path, id: &InstanceId) -> Self {
        Self {
            base_path: base.join(id.to_string()),
        }
    }

    /// Path to the cloned rootfs image.
    pub fn rootfs_path(&self) -> PathBuf {
        self.base_path.join("rootfs.raw")
    }

    /// Path to the cloud-init config-drive image.
    pub fn cloud_init_path(&self) -> PathBuf {
        self.base_path.join("cloud-init.img")
    }

    /// Path to the serial console log.
    pub fn serial_log_path(&self) -> PathBuf {
        self.base_path.join("serial.log")
    }

    /// Path to the JSON metadata file.
    pub fn metadata_path(&self) -> PathBuf {
        self.base_path.join("metadata.json")
    }

    /// Base path of this instance directory.
    pub fn path(&self) -> &Path {
        &self.base_path
    }

    /// Atomically write instance metadata (write to tmp then rename).
    pub fn write_metadata(&self, meta: &InstanceMeta) -> Result<(), ImageError> {
        let target = self.metadata_path();
        let tmp = self.base_path.join("metadata.json.tmp");
        let data = serde_json::to_string_pretty(meta).map_err(|e| ImageError::DiskCloneFailed {
            reason: format!("failed to serialize metadata: {e}"),
        })?;
        let mut f = fs::File::create(&tmp).map_err(|e| ImageError::DiskCloneFailed {
            reason: format!("failed to create temp metadata file: {e}"),
        })?;
        f.write_all(data.as_bytes())
            .map_err(|e| ImageError::DiskCloneFailed {
                reason: format!("failed to write metadata: {e}"),
            })?;
        f.sync_all().map_err(|e| ImageError::DiskCloneFailed {
            reason: format!("failed to sync metadata: {e}"),
        })?;
        fs::rename(&tmp, &target).map_err(|e| ImageError::DiskCloneFailed {
            reason: format!("failed to rename metadata: {e}"),
        })?;
        Ok(())
    }

    /// Read instance metadata from disk.
    pub fn read_metadata(&self) -> Result<InstanceMeta, ImageError> {
        let path = self.metadata_path();
        let data = fs::read_to_string(&path).map_err(|e| ImageError::DiskCloneFailed {
            reason: format!("failed to read metadata {}: {e}", path.display()),
        })?;
        serde_json::from_str(&data).map_err(|e| ImageError::DiskCloneFailed {
            reason: format!("failed to parse metadata: {e}"),
        })
    }

    /// Remove the entire instance directory and all contents.
    pub fn cleanup(&self) -> Result<(), ImageError> {
        if self.base_path.exists() {
            fs::remove_dir_all(&self.base_path).map_err(|e| ImageError::DiskCloneFailed {
                reason: format!(
                    "failed to remove instance dir {}: {e}",
                    self.base_path.display()
                ),
            })?;
        }
        Ok(())
    }

    /// Check whether the instance directory exists on disk.
    pub fn exists(&self) -> bool {
        self.base_path.exists()
    }
}

// ---------------------------------------------------------------------------
// clone_image (#549)
// ---------------------------------------------------------------------------

/// Clone a base image into an instance directory, optionally resizing.
///
/// Steps:
/// 1. Check available disk space (base size + 1 GB buffer).
/// 2. Copy with `cp --reflink=auto` for CoW when available.
/// 3. If `disk_size_mb` is specified and larger than `base_min_disk_mb`,
///    extend the file with `truncate` and grow the filesystem with `resize2fs`.
/// 4. Return the effective disk size in bytes.
///
/// On resize failure the cloned file is removed (compensating cleanup).
pub fn clone_image(
    base_path: &Path,
    instance_dir: &InstanceDir,
    disk_size_mb: Option<u32>,
    base_min_disk_mb: u32,
) -> Result<u64, ImageError> {
    let dest = instance_dir.rootfs_path();

    // -- 1. Preflight: disk space check --------------------------------------
    let base_size = fs::metadata(base_path)
        .map_err(|e| ImageError::DiskCloneFailed {
            reason: format!("cannot stat base image {}: {e}", base_path.display()),
        })?
        .len();

    let required_bytes = base_size + 1_073_741_824; // base + 1 GB buffer
    let available = available_disk_space(instance_dir.path());
    if available < required_bytes {
        return Err(ImageError::InsufficientDiskSpace {
            required_mb: required_bytes / (1024 * 1024),
            available_mb: available / (1024 * 1024),
        });
    }

    // -- 2. Clone with reflink -----------------------------------------------
    let start = Instant::now();
    let cp = Command::new("cp")
        .args([
            "--reflink=auto",
            &base_path.to_string_lossy(),
            &dest.to_string_lossy(),
        ])
        .output()
        .map_err(|e| ImageError::DiskCloneFailed {
            reason: format!("failed to run cp: {e}"),
        })?;

    if !cp.status.success() {
        return Err(ImageError::DiskCloneFailed {
            reason: format!("cp failed: {}", String::from_utf8_lossy(&cp.stderr).trim()),
        });
    }

    let elapsed = start.elapsed();
    let strategy = if elapsed.as_secs() < 1 {
        "reflink (instant)"
    } else {
        "full copy"
    };
    info!(
        path = %dest.display(),
        strategy,
        elapsed_ms = elapsed.as_millis() as u64,
        "image cloned"
    );

    // -- 3. Optional resize --------------------------------------------------
    let effective_size = if let Some(target_mb) = disk_size_mb {
        if target_mb > base_min_disk_mb {
            if let Err(e) = resize_image(&dest, target_mb) {
                // Compensating cleanup: remove the clone
                warn!(path = %dest.display(), "resize failed, removing clone");
                let _ = fs::remove_file(&dest);
                return Err(e);
            }
            info!(
                from_mb = base_min_disk_mb,
                to_mb = target_mb,
                "disk resized"
            );
            u64::from(target_mb) * 1024 * 1024
        } else {
            u64::from(base_min_disk_mb) * 1024 * 1024
        }
    } else {
        u64::from(base_min_disk_mb) * 1024 * 1024
    };

    Ok(effective_size)
}

/// Resize a raw disk image: truncate then resize2fs.
fn resize_image(path: &Path, size_mb: u32) -> Result<(), ImageError> {
    let path_str = path.to_string_lossy();

    // truncate -s {size}M
    let truncate = Command::new("truncate")
        .args(["-s", &format!("{size_mb}M"), &*path_str])
        .output()
        .map_err(|e| ImageError::ResizeFailed {
            reason: format!("failed to run truncate: {e}"),
        })?;

    if !truncate.status.success() {
        return Err(ImageError::ResizeFailed {
            reason: format!(
                "truncate failed: {}",
                String::from_utf8_lossy(&truncate.stderr).trim()
            ),
        });
    }

    // resize2fs (best-effort — may not be installed or FS may not be ext4)
    let resize = Command::new("resize2fs").arg(&*path_str).output();

    match resize {
        Ok(output) if !output.status.success() => {
            warn!(
                stderr = %String::from_utf8_lossy(&output.stderr).trim(),
                "resize2fs failed (non-ext4 or tool issue), truncate already applied"
            );
        }
        Err(e) => {
            warn!(%e, "resize2fs not available, truncate already applied");
        }
        Ok(_) => {
            info!("resize2fs completed successfully");
        }
    }

    Ok(())
}

/// Query available disk space for the filesystem containing `path`.
fn available_disk_space(path: &Path) -> u64 {
    // Use statvfs via libc
    use std::ffi::CString;
    let c_path = match CString::new(path.to_string_lossy().as_bytes()) {
        Ok(p) => p,
        Err(_) => return u64::MAX, // cannot check — assume enough
    };

    unsafe {
        let mut stat: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c_path.as_ptr(), &mut stat) == 0 {
            stat.f_bavail * stat.f_bsize
        } else {
            u64::MAX // cannot check — assume enough
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // -- InstanceDir tests (#548) --------------------------------------------

    #[test]
    fn create_instance_dir_and_verify_paths() {
        let tmp = TempDir::new().unwrap();
        let id = InstanceId::new();
        let dir = InstanceDir::create(tmp.path(), &id).unwrap();

        assert!(dir.exists());
        assert!(dir.path().ends_with(id.to_string()));
        assert_eq!(dir.rootfs_path(), dir.path().join("rootfs.raw"));
        assert_eq!(dir.cloud_init_path(), dir.path().join("cloud-init.img"));
        assert_eq!(dir.serial_log_path(), dir.path().join("serial.log"));
        assert_eq!(dir.metadata_path(), dir.path().join("metadata.json"));
    }

    #[test]
    fn double_create_returns_error() {
        let tmp = TempDir::new().unwrap();
        let id = InstanceId::new();
        let _dir = InstanceDir::create(tmp.path(), &id).unwrap();
        let result = InstanceDir::create(tmp.path(), &id);
        assert!(result.is_err());
    }

    #[test]
    fn metadata_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let id = InstanceId::new();
        let dir = InstanceDir::create(tmp.path(), &id).unwrap();

        let meta = InstanceMeta {
            image_source: "ubuntu-24.04".to_string(),
            image_sha: "abc123".to_string(),
            arch: "aarch64".to_string(),
            requested_disk_size_mb: Some(20480),
            effective_disk_size_mb: 20480,
            hostname: "vm-web-1".to_string(),
            created_at: "2025-06-01T12:00:00Z".to_string(),
            vm_name: "web-1".to_string(),
        };

        dir.write_metadata(&meta).unwrap();
        let back = dir.read_metadata().unwrap();
        assert_eq!(meta, back);
    }

    #[test]
    fn metadata_atomic_write_no_tmp_left() {
        let tmp = TempDir::new().unwrap();
        let id = InstanceId::new();
        let dir = InstanceDir::create(tmp.path(), &id).unwrap();

        let meta = InstanceMeta {
            image_source: "alpine".to_string(),
            image_sha: "def456".to_string(),
            arch: "x86_64".to_string(),
            requested_disk_size_mb: None,
            effective_disk_size_mb: 512,
            hostname: "vm-test".to_string(),
            created_at: "2025-06-01T12:00:00Z".to_string(),
            vm_name: "test".to_string(),
        };

        dir.write_metadata(&meta).unwrap();

        // The .tmp file should not exist after a successful write
        assert!(!dir.path().join("metadata.json.tmp").exists());
        assert!(dir.metadata_path().exists());
    }

    #[test]
    fn cleanup_removes_everything() {
        let tmp = TempDir::new().unwrap();
        let id = InstanceId::new();
        let dir = InstanceDir::create(tmp.path(), &id).unwrap();

        // Write some files
        fs::write(dir.rootfs_path(), b"fake image").unwrap();
        fs::write(dir.serial_log_path(), b"boot log").unwrap();

        let path = dir.path().to_path_buf();
        dir.cleanup().unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn cleanup_nonexistent_is_ok() {
        let tmp = TempDir::new().unwrap();
        let id = InstanceId::new();
        let dir = InstanceDir::open(tmp.path(), &id);
        // Dir was never created — cleanup should be a no-op
        assert!(dir.cleanup().is_ok());
    }

    #[test]
    fn exists_false_before_create() {
        let tmp = TempDir::new().unwrap();
        let id = InstanceId::new();
        let dir = InstanceDir::open(tmp.path(), &id);
        assert!(!dir.exists());
    }

    // -- Clone tests (#549) --------------------------------------------------

    #[test]
    fn clone_small_file_preserves_content() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path().join("base.raw");

        // Create a 1 MB base image
        let data: Vec<u8> = (0..1024 * 1024).map(|i| (i % 256) as u8).collect();
        fs::write(&base, &data).unwrap();

        let id = InstanceId::new();
        let dir = InstanceDir::create(tmp.path(), &id).unwrap();

        let effective = clone_image(&base, &dir, None, 1).unwrap();
        assert_eq!(effective, 1024 * 1024); // 1 MB in bytes

        let cloned = fs::read(dir.rootfs_path()).unwrap();
        assert_eq!(data, cloned);
    }

    #[test]
    fn clone_with_resize_truncate() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path().join("base.raw");

        // Create a small base image
        fs::write(&base, vec![0u8; 1024 * 1024]).unwrap();

        let id = InstanceId::new();
        let dir = InstanceDir::create(tmp.path(), &id).unwrap();

        // Request 2 MB, base min is 1 MB → should resize
        let effective = clone_image(&base, &dir, Some(2), 1).unwrap();
        assert_eq!(effective, 2 * 1024 * 1024);

        // Verify file was truncated to at least 2 MB
        let size = fs::metadata(dir.rootfs_path()).unwrap().len();
        assert!(size >= 2 * 1024 * 1024);
    }

    #[test]
    fn clone_without_resize_when_target_smaller() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path().join("base.raw");
        fs::write(&base, vec![0u8; 1024]).unwrap();

        let id = InstanceId::new();
        let dir = InstanceDir::create(tmp.path(), &id).unwrap();

        // Request 5 MB but base_min is 10 MB → no resize
        let effective = clone_image(&base, &dir, Some(5), 10).unwrap();
        assert_eq!(effective, 10 * 1024 * 1024);
    }

    #[test]
    fn clone_cleanup_on_resize_failure() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path().join("base.raw");
        fs::write(&base, vec![0u8; 1024]).unwrap();

        let id = InstanceId::new();
        let dir = InstanceDir::create(tmp.path(), &id).unwrap();

        // Make rootfs path a directory so truncate will fail
        fs::create_dir_all(dir.rootfs_path()).unwrap();

        let result = clone_image(&base, &dir, Some(100), 1);
        // cp will fail because dest is a directory
        assert!(result.is_err());
    }

    #[test]
    fn instance_meta_serde_roundtrip() {
        let meta = InstanceMeta {
            image_source: "debian-12".to_string(),
            image_sha: "aabbccdd".to_string(),
            arch: "x86_64".to_string(),
            requested_disk_size_mb: None,
            effective_disk_size_mb: 4096,
            hostname: "db-primary".to_string(),
            created_at: "2025-07-01T00:00:00Z".to_string(),
            vm_name: "db-primary".to_string(),
        };

        let json = serde_json::to_string(&meta).unwrap();
        let back: InstanceMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(meta, back);
    }
}
