//! Kernel resolution for VM boot.
//!
//! Every VM needs a kernel. Syfrah ships a bundled kernel (officially supported)
//! with an option for operators to provide their own (best-effort support).
//! The kernel mode is configured once and validated at daemon startup.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::image::error::ImageError;

/// Default path for the bundled kernel shipped with Syfrah releases.
pub const BUNDLED_KERNEL_PATH: &str = "/opt/syfrah/kernels/vmlinux";

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
}
