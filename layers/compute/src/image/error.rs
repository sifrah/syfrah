/// Typed errors for image operations.
///
/// Each variant maps to a specific failure mode with enough context for the
/// operator to diagnose and fix the issue.
#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    #[error("image not found: {name}")]
    ImageNotFound { name: String },

    #[error("image already exists: {name}. To replace it, delete first with: syfrah compute image delete {name}")]
    ImageAlreadyExists { name: String },

    #[error("catalog fetch failed for {url}: {reason}")]
    CatalogFetchFailed { url: String, reason: String },

    #[error("catalog unavailable")]
    CatalogUnavailable,

    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("invalid image format: {detail}")]
    InvalidImageFormat { detail: String },

    #[error("image {name} is in use by {vm_count} VM(s)")]
    ImageInUse { name: String, vm_count: u32 },

    #[error("import failed: {reason}")]
    ImportFailed { reason: String },

    #[error("disk clone failed: {reason}")]
    DiskCloneFailed { reason: String },

    #[error("resize failed: {reason}")]
    ResizeFailed { reason: String },

    #[error("cloud-init generation failed: {reason}")]
    CloudInitGenerationFailed { reason: String },

    #[error("insufficient disk space: required {required_mb} MB, available {available_mb} MB")]
    InsufficientDiskSpace { required_mb: u64, available_mb: u64 },

    #[error("architecture mismatch: image is {image_arch}, node is {node_arch}")]
    ArchMismatch {
        image_arch: String,
        node_arch: String,
    },

    #[error("kernel not found: {path}")]
    KernelNotFound { path: String },

    #[error("invalid image name: {reason}")]
    InvalidImageName { reason: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ComputeError;

    // -- Display tests for every variant --------------------------------------

    #[test]
    fn display_image_not_found() {
        let e = ImageError::ImageNotFound {
            name: "ubuntu-24.04".to_string(),
        };
        assert!(e.to_string().contains("ubuntu-24.04"));
    }

    #[test]
    fn display_image_already_exists() {
        let e = ImageError::ImageAlreadyExists {
            name: "alpine".to_string(),
        };
        assert!(e.to_string().contains("alpine"));
        assert!(e.to_string().contains("already exists"));
    }

    #[test]
    fn display_catalog_fetch_failed() {
        let e = ImageError::CatalogFetchFailed {
            url: "https://images.syfrah.dev".to_string(),
            reason: "DNS failure".to_string(),
        };
        let msg = e.to_string();
        assert!(msg.contains("https://images.syfrah.dev"));
        assert!(msg.contains("DNS failure"));
    }

    #[test]
    fn display_catalog_unavailable() {
        let e = ImageError::CatalogUnavailable;
        assert!(e.to_string().contains("catalog unavailable"));
    }

    #[test]
    fn display_checksum_mismatch() {
        let e = ImageError::ChecksumMismatch {
            expected: "abc123".to_string(),
            actual: "def456".to_string(),
        };
        let msg = e.to_string();
        assert!(msg.contains("abc123"));
        assert!(msg.contains("def456"));
    }

    #[test]
    fn display_invalid_image_format() {
        let e = ImageError::InvalidImageFormat {
            detail: "unsupported format vdi".to_string(),
        };
        assert!(e.to_string().contains("unsupported format vdi"));
    }

    #[test]
    fn display_image_in_use() {
        let e = ImageError::ImageInUse {
            name: "debian-12".to_string(),
            vm_count: 3,
        };
        let msg = e.to_string();
        assert!(msg.contains("debian-12"));
        assert!(msg.contains("3"));
    }

    #[test]
    fn display_import_failed() {
        let e = ImageError::ImportFailed {
            reason: "corrupted archive".to_string(),
        };
        assert!(e.to_string().contains("corrupted archive"));
    }

    #[test]
    fn display_disk_clone_failed() {
        let e = ImageError::DiskCloneFailed {
            reason: "I/O error".to_string(),
        };
        assert!(e.to_string().contains("I/O error"));
    }

    #[test]
    fn display_resize_failed() {
        let e = ImageError::ResizeFailed {
            reason: "filesystem busy".to_string(),
        };
        assert!(e.to_string().contains("filesystem busy"));
    }

    #[test]
    fn display_cloud_init_generation_failed() {
        let e = ImageError::CloudInitGenerationFailed {
            reason: "template error".to_string(),
        };
        assert!(e.to_string().contains("template error"));
    }

    #[test]
    fn display_insufficient_disk_space() {
        let e = ImageError::InsufficientDiskSpace {
            required_mb: 8192,
            available_mb: 2048,
        };
        let msg = e.to_string();
        assert!(msg.contains("8192"));
        assert!(msg.contains("2048"));
    }

    #[test]
    fn display_arch_mismatch() {
        let e = ImageError::ArchMismatch {
            image_arch: "aarch64".to_string(),
            node_arch: "x86_64".to_string(),
        };
        let msg = e.to_string();
        assert!(msg.contains("aarch64"));
        assert!(msg.contains("x86_64"));
    }

    #[test]
    fn display_kernel_not_found() {
        let e = ImageError::KernelNotFound {
            path: "/opt/syfrah/vmlinux".to_string(),
        };
        assert!(e.to_string().contains("/opt/syfrah/vmlinux"));
    }

    #[test]
    fn display_invalid_image_name() {
        let e = ImageError::InvalidImageName {
            reason: "must not contain '..'".to_string(),
        };
        assert!(e.to_string().contains("invalid image name"));
        assert!(e.to_string().contains("must not contain"));
    }

    // -- From impl tests ------------------------------------------------------

    #[test]
    fn compute_error_from_image_error() {
        let inner = ImageError::CatalogUnavailable;
        let outer: ComputeError = inner.into();
        assert!(matches!(outer, ComputeError::Image(_)));
        assert!(outer.to_string().contains("image"));
    }
}
