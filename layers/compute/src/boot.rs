//! Kernel resolution for VM boot.
//!
//! Every VM needs a kernel. Syfrah ships a bundled kernel (officially supported)
//! with an option for operators to provide their own (best-effort support).
//! The kernel mode is configured once and validated at daemon startup.

use std::io::Read;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::image::error::ImageError;

/// Default path for the bundled kernel shipped with Syfrah releases.
pub const BUNDLED_KERNEL_PATH: &str = "/opt/syfrah/kernels/vmlinux";

/// URL for downloading the kernel when it is not present locally.
const KERNEL_DOWNLOAD_URL: &str =
    "https://github.com/sacha-ops/syfrah-images/releases/latest/download/vmlinux.gz";

/// Kernel mode determines where the VM kernel comes from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KernelMode {
    /// Use the officially bundled kernel shipped with Syfrah.
    #[default]
    Bundled,
    /// Use an operator-provided kernel at a custom path.
    Custom,
}

/// Kernel configuration from `[compute.kernel]` in config.toml.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KernelConfig {
    /// Which kernel to use.
    #[serde(default)]
    pub mode: KernelMode,
    /// Path to a custom kernel. Required when mode is `Custom`, ignored for `Bundled`.
    pub path: Option<PathBuf>,
}

/// Resolve the kernel path based on the provided configuration.
///
/// - **Bundled**: checks `/opt/syfrah/kernels/vmlinux`
/// - **Custom**: uses the path from config (required; errors if `None`)
///
/// The resolved path is validated to exist and be readable. Returns
/// [`ImageError::KernelNotFound`] with a helpful message if the file is missing.
pub fn resolve_kernel(config: &KernelConfig) -> Result<PathBuf, ImageError> {
    resolve_kernel_inner(config, BUNDLED_KERNEL_PATH)
}

/// Ensure the kernel is available, downloading it if necessary.
///
/// If the kernel file already exists, this returns its path immediately.
/// If the kernel is missing and the mode is `Bundled`, downloads the kernel
/// from the syfrah-images GitHub release, decompresses it, and saves it to
/// the default kernel path.
///
/// This is intended to be called at daemon startup so operators never have
/// to manually provision the kernel.
pub async fn ensure_kernel(config: &KernelConfig) -> Result<PathBuf, ImageError> {
    ensure_kernel_inner(config, BUNDLED_KERNEL_PATH, KERNEL_DOWNLOAD_URL).await
}

/// Inner implementation that accepts configurable paths (for testing).
async fn ensure_kernel_inner(
    config: &KernelConfig,
    bundled_path: &str,
    download_url: &str,
) -> Result<PathBuf, ImageError> {
    let kernel_path = match &config.mode {
        KernelMode::Bundled => PathBuf::from(bundled_path),
        KernelMode::Custom => {
            return config
                .path
                .clone()
                .ok_or_else(|| ImageError::KernelNotFound {
                    path: "kernel path is required when mode is 'custom'. \
                           Set [compute.kernel].path in config.toml"
                        .to_string(),
                })
                .and_then(|p| {
                    validate_kernel_path(&p, &config.mode)?;
                    Ok(p)
                });
        }
    };

    if kernel_path.exists() {
        info!("Kernel already present at {}", kernel_path.display());
        return Ok(kernel_path);
    }

    warn!("Kernel not found locally, downloading from syfrah-images...");

    let url = download_url.to_string();
    let dest = kernel_path.clone();

    tokio::task::spawn_blocking(move || download_and_decompress_kernel(&url, &dest))
        .await
        .map_err(|e| ImageError::KernelNotFound {
            path: format!("kernel download task panicked: {e}"),
        })??;

    info!(
        "Kernel downloaded and installed to {}",
        kernel_path.display()
    );
    Ok(kernel_path)
}

/// Download a gzip-compressed kernel and decompress it to `dest`.
fn download_and_decompress_kernel(url: &str, dest: &Path) -> Result<(), ImageError> {
    let response = reqwest::blocking::get(url).map_err(|e| ImageError::KernelNotFound {
        path: format!("failed to download kernel from {url}: {e}"),
    })?;

    if !response.status().is_success() {
        return Err(ImageError::KernelNotFound {
            path: format!(
                "failed to download kernel from {url}: HTTP {}",
                response.status()
            ),
        });
    }

    let bytes = response.bytes().map_err(|e| ImageError::KernelNotFound {
        path: format!("failed to read kernel response body: {e}"),
    })?;

    let mut decoder = GzDecoder::new(&bytes[..]);
    let mut decompressed = Vec::new();
    decoder
        .read_to_end(&mut decompressed)
        .map_err(|e| ImageError::KernelNotFound {
            path: format!("failed to decompress kernel: {e}"),
        })?;

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| ImageError::KernelNotFound {
            path: format!(
                "failed to create kernel directory {}: {e}",
                parent.display()
            ),
        })?;
    }

    std::fs::write(dest, &decompressed).map_err(|e| ImageError::KernelNotFound {
        path: format!("failed to write kernel to {}: {e}", dest.display()),
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o644);
        std::fs::set_permissions(dest, perms).map_err(|e| ImageError::KernelNotFound {
            path: format!("failed to set permissions on {}: {e}", dest.display()),
        })?;
    }

    Ok(())
}

/// Inner implementation that accepts a configurable bundled path (for testing).
fn resolve_kernel_inner(config: &KernelConfig, bundled_path: &str) -> Result<PathBuf, ImageError> {
    let kernel_path = match &config.mode {
        KernelMode::Bundled => PathBuf::from(bundled_path),
        KernelMode::Custom => config
            .path
            .clone()
            .ok_or_else(|| ImageError::KernelNotFound {
                path: "kernel path is required when mode is 'custom'. \
                       Set [compute.kernel].path in config.toml"
                    .to_string(),
            })?,
    };

    validate_kernel_path(&kernel_path, &config.mode)?;

    info!(
        "Kernel resolved: {} at {}",
        match &config.mode {
            KernelMode::Bundled => "bundled",
            KernelMode::Custom => "custom",
        },
        kernel_path.display()
    );

    Ok(kernel_path)
}

/// Validate that the kernel file exists and is readable.
fn validate_kernel_path(path: &Path, mode: &KernelMode) -> Result<(), ImageError> {
    if !path.exists() {
        let hint = match mode {
            KernelMode::Bundled => format!(
                "{} not found. Install the bundled kernel with \
                 'syfrah compute image pull --kernel' or reinstall Syfrah.",
                path.display()
            ),
            KernelMode::Custom => format!(
                "{} not found. Verify the path in [compute.kernel].path \
                 or switch to mode = 'bundled'.",
                path.display()
            ),
        };
        return Err(ImageError::KernelNotFound { path: hint });
    }

    // Check readable by attempting to open the file.
    std::fs::File::open(path).map_err(|e| ImageError::KernelNotFound {
        path: format!("{}: {}", path.display(), e),
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Helper: create a fake kernel file in a temp directory.
    fn create_kernel(dir: &TempDir, name: &str) -> PathBuf {
        let path = dir.path().join(name);
        fs::write(&path, b"FAKE_ELF_KERNEL").unwrap();
        path
    }

    #[test]
    fn bundled_kernel_found_in_tmpdir() {
        let dir = TempDir::new().unwrap();
        let kernel = create_kernel(&dir, "vmlinux");
        let config = KernelConfig {
            mode: KernelMode::Bundled,
            path: None,
        };
        let result = resolve_kernel_inner(&config, kernel.to_str().unwrap());
        assert_eq!(result.unwrap(), kernel);
    }

    #[test]
    fn bundled_kernel_missing_returns_kernel_not_found() {
        let config = KernelConfig {
            mode: KernelMode::Bundled,
            path: None,
        };
        let result = resolve_kernel_inner(&config, "/nonexistent/vmlinux");
        let err = result.unwrap_err();
        match &err {
            ImageError::KernelNotFound { path } => {
                assert!(path.contains("/nonexistent/vmlinux"));
                assert!(path.contains("syfrah compute image pull --kernel"));
            }
            other => panic!("expected KernelNotFound, got: {other:?}"),
        }
    }

    #[test]
    fn custom_kernel_found() {
        let dir = TempDir::new().unwrap();
        let kernel = create_kernel(&dir, "my-kernel");
        let config = KernelConfig {
            mode: KernelMode::Custom,
            path: Some(kernel.clone()),
        };
        let result = resolve_kernel_inner(&config, BUNDLED_KERNEL_PATH);
        assert_eq!(result.unwrap(), kernel);
    }

    #[test]
    fn custom_kernel_missing_returns_kernel_not_found() {
        let config = KernelConfig {
            mode: KernelMode::Custom,
            path: Some(PathBuf::from("/tmp/does-not-exist-kernel")),
        };
        let result = resolve_kernel_inner(&config, BUNDLED_KERNEL_PATH);
        let err = result.unwrap_err();
        match &err {
            ImageError::KernelNotFound { path } => {
                assert!(path.contains("/tmp/does-not-exist-kernel"));
            }
            other => panic!("expected KernelNotFound, got: {other:?}"),
        }
    }

    #[test]
    fn custom_mode_without_path_returns_error() {
        let config = KernelConfig {
            mode: KernelMode::Custom,
            path: None,
        };
        let result = resolve_kernel_inner(&config, BUNDLED_KERNEL_PATH);
        let err = result.unwrap_err();
        match &err {
            ImageError::KernelNotFound { path } => {
                assert!(path.contains("required"));
            }
            other => panic!("expected KernelNotFound, got: {other:?}"),
        }
    }

    #[test]
    fn default_kernel_mode_is_bundled() {
        let mode = KernelMode::default();
        assert_eq!(mode, KernelMode::Bundled);
    }

    #[test]
    fn default_kernel_config_is_bundled_with_no_path() {
        let config = KernelConfig::default();
        assert_eq!(config.mode, KernelMode::Bundled);
        assert!(config.path.is_none());
    }

    #[test]
    fn kernel_config_serde_roundtrip() {
        let config = KernelConfig {
            mode: KernelMode::Custom,
            path: Some(PathBuf::from("/opt/my-kernel")),
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: KernelConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.mode, KernelMode::Custom);
        assert_eq!(deserialized.path.unwrap(), PathBuf::from("/opt/my-kernel"));
    }

    #[test]
    fn kernel_mode_serde_roundtrip() {
        for mode in [KernelMode::Bundled, KernelMode::Custom] {
            let json = serde_json::to_string(&mode).unwrap();
            let deserialized: KernelMode = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, mode);
        }
    }

    #[test]
    fn kernel_mode_serde_values() {
        assert_eq!(
            serde_json::to_string(&KernelMode::Bundled).unwrap(),
            "\"bundled\""
        );
        assert_eq!(
            serde_json::to_string(&KernelMode::Custom).unwrap(),
            "\"custom\""
        );
    }

    // -- Additional boot asset resolution tests (issue #542) ------------------

    #[test]
    fn bundled_kernel_missing_error_contains_path() {
        let config = KernelConfig {
            mode: KernelMode::Bundled,
            path: None,
        };
        let result = resolve_kernel_inner(&config, "/opt/syfrah/kernels/vmlinux");
        let err = result.unwrap_err();
        match &err {
            ImageError::KernelNotFound { path } => {
                assert!(
                    path.contains("/opt/syfrah/kernels/vmlinux"),
                    "error should contain the missing path, got: {path}"
                );
            }
            other => panic!("expected KernelNotFound, got: {other:?}"),
        }
    }

    #[test]
    fn custom_kernel_missing_error_suggests_switching_mode() {
        let config = KernelConfig {
            mode: KernelMode::Custom,
            path: Some(PathBuf::from("/srv/kernels/custom-vmlinux")),
        };
        let result = resolve_kernel_inner(&config, BUNDLED_KERNEL_PATH);
        let err = result.unwrap_err();
        match &err {
            ImageError::KernelNotFound { path } => {
                assert!(
                    path.contains("bundled"),
                    "custom-not-found error should suggest switching to bundled, got: {path}"
                );
            }
            other => panic!("expected KernelNotFound, got: {other:?}"),
        }
    }

    #[test]
    fn bundled_resolved_path_equals_bundled_path() {
        let dir = TempDir::new().unwrap();
        let kernel = create_kernel(&dir, "vmlinux");
        let bundled = kernel.to_str().unwrap();
        let config = KernelConfig {
            mode: KernelMode::Bundled,
            path: Some(PathBuf::from("/ignored/path")),
        };
        let result = resolve_kernel_inner(&config, bundled).unwrap();
        assert_eq!(result, kernel);
    }

    #[test]
    fn kernel_config_serde_default_omits_path() {
        let config = KernelConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["mode"], "bundled");
        assert!(parsed["path"].is_null());
    }

    #[test]
    fn kernel_config_deserialize_from_minimal_json() {
        let json = r#"{"mode": "bundled"}"#;
        let config: KernelConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.mode, KernelMode::Bundled);
        assert!(config.path.is_none());
    }

    #[test]
    fn kernel_config_deserialize_empty_defaults_to_bundled() {
        let json = r#"{}"#;
        let config: KernelConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.mode, KernelMode::Bundled);
        assert!(config.path.is_none());
    }

    #[test]
    fn symlink_to_kernel_resolves_successfully() {
        let dir = TempDir::new().unwrap();
        let real_kernel = create_kernel(&dir, "vmlinux-real");
        let symlink_path = dir.path().join("vmlinux-link");
        std::os::unix::fs::symlink(&real_kernel, &symlink_path).unwrap();

        let config = KernelConfig {
            mode: KernelMode::Custom,
            path: Some(symlink_path.clone()),
        };
        let result = resolve_kernel_inner(&config, BUNDLED_KERNEL_PATH).unwrap();
        assert_eq!(result, symlink_path);
    }

    #[test]
    fn bundled_constant_points_to_expected_path() {
        assert_eq!(BUNDLED_KERNEL_PATH, "/opt/syfrah/kernels/vmlinux");
    }

    // -- ensure_kernel tests (issue #600) -------------------------------------

    #[tokio::test]
    async fn ensure_kernel_returns_existing_kernel() {
        let dir = TempDir::new().unwrap();
        let kernel = create_kernel(&dir, "vmlinux");
        let config = KernelConfig {
            mode: KernelMode::Bundled,
            path: None,
        };
        let result = ensure_kernel_inner(&config, kernel.to_str().unwrap(), "http://invalid").await;
        assert_eq!(result.unwrap(), kernel);
    }

    #[tokio::test]
    async fn ensure_kernel_custom_mode_validates_path() {
        let config = KernelConfig {
            mode: KernelMode::Custom,
            path: None,
        };
        let result = ensure_kernel_inner(&config, BUNDLED_KERNEL_PATH, "http://invalid").await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ImageError::KernelNotFound { path } => {
                assert!(path.contains("required"));
            }
            other => panic!("expected KernelNotFound, got: {other:?}"),
        }
    }
}
