use std::fs;
use std::path::Path;

use crate::config::ResolvedSpec;
use crate::error::PreflightError;

/// Run all preflight checks against a resolved VM spec.
///
/// Collects ALL errors rather than failing on the first one, so operators see
/// everything that needs fixing in a single pass.
pub fn run_preflight(
    spec: &ResolvedSpec,
    ch_binary: &Path,
    socket_path: &Path,
) -> Result<(), Vec<PreflightError>> {
    let mut errors = Vec::new();

    // Base checks — always run
    if let Some(e) = check_ch_binary(ch_binary) {
        errors.push(e);
    }
    if let Some(e) = check_kvm() {
        errors.push(e);
    }
    if let Some(e) = check_kernel(&spec.kernel_path) {
        errors.push(e);
    }
    if let Some(e) = check_image(&spec.rootfs_path) {
        errors.push(e);
    }
    if let Some(e) = check_socket_path(socket_path) {
        errors.push(e);
    }
    if let Some(e) = check_cgroup_v2() {
        errors.push(e);
    }

    // Conditional: VFIO check only when GPU passthrough is requested
    if let Some(ref gpu_path) = spec.gpu_sysfs_path {
        if let Some(e) = check_vfio_path(gpu_path) {
            errors.push(e);
        }
    }

    // Capacity check
    if let Some(e) = check_capacity(spec.vcpus, spec.memory_mb) {
        errors.push(e);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

// ---------------------------------------------------------------------------
// Base checks (#470)
// ---------------------------------------------------------------------------

/// Check that the Cloud Hypervisor binary exists and is executable.
fn check_ch_binary(path: &Path) -> Option<PreflightError> {
    match fs::metadata(path) {
        Ok(meta) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = meta.permissions().mode();
                if mode & 0o111 == 0 {
                    return Some(PreflightError::ChBinaryNotFound);
                }
            }
            let _ = meta;
            None
        }
        Err(_) => Some(PreflightError::ChBinaryNotFound),
    }
}

/// Check that `/dev/kvm` exists and is readable.
fn check_kvm() -> Option<PreflightError> {
    match fs::metadata("/dev/kvm") {
        Ok(_) => {
            // Try opening for read to verify access permissions
            match fs::File::open("/dev/kvm") {
                Ok(_) => None,
                Err(_) => Some(PreflightError::KvmNotAvailable),
            }
        }
        Err(_) => Some(PreflightError::KvmNotAvailable),
    }
}

/// Check that the kernel image file exists.
fn check_kernel(path: &Path) -> Option<PreflightError> {
    if path.exists() {
        None
    } else {
        Some(PreflightError::KernelNotFound)
    }
}

/// Check that the rootfs disk image file exists.
fn check_image(path: &Path) -> Option<PreflightError> {
    if path.exists() {
        None
    } else {
        Some(PreflightError::ImageNotFound)
    }
}

// ---------------------------------------------------------------------------
// Advanced checks (#472)
// ---------------------------------------------------------------------------

/// Check that a VFIO device is properly bound at the given sysfs path.
///
/// Verifies:
/// 1. The PCI device directory exists
/// 2. The `driver` symlink points to `vfio-pci`
fn check_vfio_path(gpu_path: &Path) -> Option<PreflightError> {
    let bdf = extract_bdf_from_sysfs(gpu_path);

    if !gpu_path.exists() {
        return Some(PreflightError::VfioNotBound { bdf });
    }

    let driver_link = gpu_path.join("driver");
    match fs::read_link(&driver_link) {
        Ok(target) => {
            // The symlink target is something like ../../../../bus/pci/drivers/vfio-pci
            // We only care about the last component.
            let driver_name = target.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if driver_name == "vfio-pci" {
                None
            } else {
                Some(PreflightError::VfioNotBound { bdf })
            }
        }
        Err(_) => Some(PreflightError::VfioNotBound { bdf }),
    }
}

/// Check that cgroup v2 is available on the host.
fn check_cgroup_v2() -> Option<PreflightError> {
    if Path::new("/sys/fs/cgroup/cgroup.controllers").exists() {
        None
    } else {
        Some(PreflightError::CgroupV2NotAvailable)
    }
}

/// Check that the socket path does NOT already exist (stale socket detection).
fn check_socket_path(path: &Path) -> Option<PreflightError> {
    if path.exists() {
        Some(PreflightError::SocketPathOccupied {
            path: path.to_string_lossy().into_owned(),
        })
    } else {
        None
    }
}

/// Check that the host has sufficient CPU and memory capacity for the VM.
///
/// Reads `/proc/cpuinfo` for CPU count and `/proc/meminfo` for available memory.
fn check_capacity(vcpus: u32, memory_mb: u32) -> Option<PreflightError> {
    // Check CPUs
    if let Some(e) = check_cpu_capacity(vcpus) {
        return Some(e);
    }

    // Check memory
    if let Some(e) = check_memory_capacity(memory_mb) {
        return Some(e);
    }

    None
}

/// Count CPUs from `/proc/cpuinfo` and compare against required vCPUs.
fn check_cpu_capacity(vcpus: u32) -> Option<PreflightError> {
    let cpuinfo = match fs::read_to_string("/proc/cpuinfo") {
        Ok(s) => s,
        Err(_) => {
            // Cannot read cpuinfo — skip check rather than block the spawn
            return None;
        }
    };

    let cpu_count = cpuinfo
        .lines()
        .filter(|l| l.starts_with("processor"))
        .count() as u32;

    if cpu_count == 0 {
        // Parsing failed, skip check
        return None;
    }

    if vcpus > cpu_count {
        Some(PreflightError::InsufficientResources {
            resource: "vcpus".to_string(),
            available: cpu_count.to_string(),
            required: vcpus.to_string(),
        })
    } else {
        None
    }
}

/// Read available memory from `/proc/meminfo` and compare against required MB.
fn check_memory_capacity(memory_mb: u32) -> Option<PreflightError> {
    let meminfo = match fs::read_to_string("/proc/meminfo") {
        Ok(s) => s,
        Err(_) => {
            // Cannot read meminfo — skip check rather than block the spawn
            return None;
        }
    };

    // Look for "MemAvailable:" line, value is in kB
    let available_kb = meminfo
        .lines()
        .find(|l| l.starts_with("MemAvailable:"))
        .and_then(|line| {
            line.split_whitespace()
                .nth(1)
                .and_then(|v| v.parse::<u64>().ok())
        });

    let available_kb = available_kb?;

    let available_mb = available_kb / 1024;
    let required_mb = u64::from(memory_mb);

    if required_mb > available_mb {
        Some(PreflightError::InsufficientResources {
            resource: "memory_mb".to_string(),
            available: available_mb.to_string(),
            required: required_mb.to_string(),
        })
    } else {
        None
    }
}

/// Extract BDF string from a sysfs path like `/sys/bus/pci/devices/0000:01:00.0/`.
fn extract_bdf_from_sysfs(path: &Path) -> String {
    // Strip trailing slash by getting the file_name component
    path.file_name()
        .or_else(|| {
            // If path ends in '/', parent's file_name is the BDF
            path.parent().and_then(|p| p.file_name())
        })
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    // -- check_ch_binary ------------------------------------------------------

    #[test]
    fn ch_binary_missing_returns_error() {
        let result = check_ch_binary(Path::new("/nonexistent/cloud-hypervisor"));
        assert!(matches!(result, Some(PreflightError::ChBinaryNotFound)));
    }

    #[test]
    fn ch_binary_exists_not_executable_returns_error() {
        let dir = TempDir::new().unwrap();
        let bin = dir.path().join("cloud-hypervisor");
        fs::write(&bin, b"fake").unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o644)).unwrap();
        let result = check_ch_binary(&bin);
        assert!(matches!(result, Some(PreflightError::ChBinaryNotFound)));
    }

    #[test]
    fn ch_binary_exists_and_executable_returns_none() {
        let dir = TempDir::new().unwrap();
        let bin = dir.path().join("cloud-hypervisor");
        fs::write(&bin, b"fake").unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
        let result = check_ch_binary(&bin);
        assert!(result.is_none());
    }

    // -- check_kernel / check_image -------------------------------------------

    #[test]
    fn kernel_missing_returns_error() {
        let result = check_kernel(Path::new("/nonexistent/vmlinux"));
        assert!(matches!(result, Some(PreflightError::KernelNotFound)));
    }

    #[test]
    fn kernel_exists_returns_none() {
        let dir = TempDir::new().unwrap();
        let kernel = dir.path().join("vmlinux");
        fs::write(&kernel, b"kernel").unwrap();
        assert!(check_kernel(&kernel).is_none());
    }

    #[test]
    fn image_missing_returns_error() {
        let result = check_image(Path::new("/nonexistent/rootfs.raw"));
        assert!(matches!(result, Some(PreflightError::ImageNotFound)));
    }

    #[test]
    fn image_exists_returns_none() {
        let dir = TempDir::new().unwrap();
        let img = dir.path().join("rootfs.raw");
        fs::write(&img, b"image").unwrap();
        assert!(check_image(&img).is_none());
    }

    // -- check_socket_path ----------------------------------------------------

    #[test]
    fn socket_path_free_returns_none() {
        let result = check_socket_path(Path::new("/nonexistent/api.sock"));
        assert!(result.is_none());
    }

    #[test]
    fn socket_path_occupied_returns_error() {
        let dir = TempDir::new().unwrap();
        let sock = dir.path().join("api.sock");
        fs::write(&sock, b"stale").unwrap();
        let result = check_socket_path(&sock);
        assert!(matches!(
            result,
            Some(PreflightError::SocketPathOccupied { .. })
        ));
    }

    // -- check_cgroup_v2 ------------------------------------------------------

    #[test]
    fn cgroup_v2_check_returns_something() {
        // On cgroup v2 systems this returns None, on others Some.
        // Just verify it doesn't panic.
        let result = check_cgroup_v2();
        if Path::new("/sys/fs/cgroup/cgroup.controllers").exists() {
            assert!(result.is_none());
        } else {
            assert!(matches!(result, Some(PreflightError::CgroupV2NotAvailable)));
        }
    }

    // -- check_vfio_path ------------------------------------------------------

    #[test]
    fn vfio_nonexistent_device_returns_error() {
        let result = check_vfio_path(Path::new("/sys/bus/pci/devices/9999:99:99.9/"));
        assert!(matches!(result, Some(PreflightError::VfioNotBound { .. })));
    }

    #[test]
    fn vfio_device_without_driver_symlink_returns_error() {
        let dir = TempDir::new().unwrap();
        let device_dir = dir.path().join("0000:01:00.0");
        fs::create_dir_all(&device_dir).unwrap();
        let result = check_vfio_path(&device_dir);
        assert!(matches!(result, Some(PreflightError::VfioNotBound { .. })));
    }

    #[test]
    fn vfio_device_with_wrong_driver_returns_error() {
        let dir = TempDir::new().unwrap();
        let device_dir = dir.path().join("0000:01:00.0");
        fs::create_dir_all(&device_dir).unwrap();
        // Create a symlink pointing to a non-vfio driver
        std::os::unix::fs::symlink("/fake/drivers/nvidia", device_dir.join("driver")).unwrap();
        let result = check_vfio_path(&device_dir);
        assert!(matches!(result, Some(PreflightError::VfioNotBound { .. })));
    }

    #[test]
    fn vfio_device_with_correct_driver_returns_none() {
        let dir = TempDir::new().unwrap();
        let device_dir = dir.path().join("0000:01:00.0");
        fs::create_dir_all(&device_dir).unwrap();
        std::os::unix::fs::symlink("/fake/drivers/vfio-pci", device_dir.join("driver")).unwrap();
        let result = check_vfio_path(&device_dir);
        assert!(result.is_none());
    }

    // -- check_capacity -------------------------------------------------------

    #[test]
    fn capacity_reasonable_request_passes() {
        // Request 1 vCPU and 64 MB — should pass on any machine running tests
        let result = check_capacity(1, 64);
        assert!(result.is_none());
    }

    #[test]
    fn capacity_excessive_vcpus_fails() {
        // Request 100_000 vCPUs — no machine has this many
        let result = check_capacity(100_000, 64);
        assert!(matches!(
            result,
            Some(PreflightError::InsufficientResources { .. })
        ));
    }

    #[test]
    fn capacity_excessive_memory_fails() {
        // Request 100 TB of RAM — no machine has this
        let result = check_capacity(1, 100_000_000);
        assert!(matches!(
            result,
            Some(PreflightError::InsufficientResources { .. })
        ));
    }

    // -- extract_bdf_from_sysfs -----------------------------------------------

    #[test]
    fn extract_bdf_from_sysfs_with_trailing_slash() {
        let path = Path::new("/sys/bus/pci/devices/0000:01:00.0/");
        assert_eq!(extract_bdf_from_sysfs(path), "0000:01:00.0");
    }

    #[test]
    fn extract_bdf_from_sysfs_without_trailing_slash() {
        let path = Path::new("/sys/bus/pci/devices/0000:01:00.0");
        assert_eq!(extract_bdf_from_sysfs(path), "0000:01:00.0");
    }

    // -- run_preflight (integration) ------------------------------------------

    #[test]
    fn run_preflight_collects_multiple_errors() {
        use crate::types::VmId;
        use std::path::PathBuf;

        let spec = ResolvedSpec {
            vm_id: VmId("vm-test".to_string()),
            vcpus: 1,
            memory_mb: 128,
            kernel_path: PathBuf::from("/nonexistent/vmlinux"),
            rootfs_path: PathBuf::from("/nonexistent/rootfs.raw"),
            network: None,
            volume_paths: vec![],
            gpu_sysfs_path: None,
        };

        let errors = run_preflight(
            &spec,
            Path::new("/nonexistent/cloud-hypervisor"),
            Path::new("/nonexistent/api.sock"),
        )
        .unwrap_err();

        // Should have at least kernel + image errors (CH binary too, but depends on host)
        assert!(
            errors.len() >= 2,
            "expected multiple errors, got {}: {:?}",
            errors.len(),
            errors
        );

        // Verify kernel and image errors are present
        assert!(errors
            .iter()
            .any(|e| matches!(e, PreflightError::KernelNotFound)));
        assert!(errors
            .iter()
            .any(|e| matches!(e, PreflightError::ImageNotFound)));
        assert!(errors
            .iter()
            .any(|e| matches!(e, PreflightError::ChBinaryNotFound)));
    }
}
