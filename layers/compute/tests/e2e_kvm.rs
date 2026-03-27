//! KVM-based E2E tests for the compute layer.
//!
//! These tests exercise real Cloud Hypervisor VMs on a KVM-capable host.
//! They do NOT run in CI — all tests are `#[ignore]`d by default.
//!
//! # Prerequisites
//!
//! 1. A Linux host with `/dev/kvm` accessible
//! 2. Cloud Hypervisor binary installed (matching `CLOUD_HYPERVISOR_VERSION`)
//! 3. Run `./scripts/setup-compute-e2e.sh` to download kernel + rootfs assets
//! 4. Set environment variables:
//!    - `SYFRAH_E2E_KERNEL` — path to the hypervisor firmware / vmlinux
//!    - `SYFRAH_E2E_ROOTFS` — path to the root filesystem image
//! 5. Root privileges (for KVM, cgroup, TAP device creation)
//!
//! # Running
//!
//! ```bash
//! cargo test -p syfrah-compute -- --ignored
//! ```

/// Full happy path: create a VM, boot it, query info, shut it down, and delete it.
///
/// Verifies:
/// - VM reaches Running phase after boot
/// - `info()` returns correct vcpus and memory
/// - `shutdown_graceful()` succeeds within 30s
/// - VM reaches Stopped phase
/// - `delete()` cleans up all artifacts (runtime dir, cgroup)
#[test]
#[ignore]
fn test_create_boot_shutdown_delete() {
    // TODO: implement when running on KVM-capable host
    //
    // 1. Create VmManager
    // 2. Create VM with 1 vCPU, 256 MB, test rootfs
    // 3. Boot the VM
    // 4. Assert phase == Running
    // 5. Query info, assert vcpus == 1, memory == 256 MB
    // 6. Shutdown gracefully (30s timeout)
    // 7. Assert phase == Stopped
    // 8. Delete VM
    // 9. Assert runtime directory is gone
}

/// Boot a VM, reboot it, and verify it returns to Running.
///
/// Verifies:
/// - Reboot command succeeds
/// - VM transitions back to Running after reboot
#[test]
#[ignore]
fn test_boot_reboot() {
    // TODO: implement when running on KVM-capable host
    //
    // 1. Create and boot a VM
    // 2. Assert Running
    // 3. Call reboot()
    // 4. Wait for Running again (with timeout)
    // 5. Assert phase == Running
}

/// Boot a VM, pause it, verify paused, resume it, verify running again.
///
/// Verifies:
/// - Pause transitions to Paused phase
/// - Resume transitions back to Running phase
#[test]
#[ignore]
fn test_boot_pause_resume() {
    // TODO: implement when running on KVM-capable host
    //
    // 1. Create and boot a VM
    // 2. Pause the VM
    // 3. Assert phase == Paused
    // 4. Resume the VM
    // 5. Assert phase == Running
}

/// Kill the syfrah/test process (NOT the CH process), create a new VmManager,
/// and call reconnect(). The VM should be discovered and recovered.
///
/// Verifies:
/// - ReconnectSucceeded event emitted
/// - VM is recovered with Running state
/// - `info()` returns correct details
/// - The original CH process PID is unchanged
#[test]
#[ignore]
fn test_daemon_restart_recovery() {
    // TODO: implement when running on KVM-capable host
    //
    // 1. Create and boot a VM, record CH PID
    // 2. Drop the VmManager (simulating daemon death)
    // 3. Create a new VmManager
    // 4. Call reconnect()
    // 5. Assert VM is recovered and Running
    // 6. Assert CH PID is the same as before
}

/// Kill both the test process and the CH process, then reconnect.
/// The VM should be marked as Failed.
///
/// Verifies:
/// - ReconnectFailed event emitted
/// - VM shows as Failed
/// - Cleanup is done or deferred
#[test]
#[ignore]
fn test_daemon_restart_with_dead_vm() {
    // TODO: implement when running on KVM-capable host
    //
    // 1. Create and boot a VM, record CH PID
    // 2. Kill the CH process
    // 3. Drop VmManager
    // 4. Create a new VmManager, call reconnect()
    // 5. Assert VM phase == Failed
}

/// Create a VM with 1 vCPU, boot it, resize to 2 vCPUs.
///
/// Verifies:
/// - Resize command succeeds
/// - `info()` reports 2 vCPUs after resize
#[test]
#[ignore]
fn test_cpu_resize() {
    // TODO: implement when running on KVM-capable host
    //
    // 1. Create VM with 1 vCPU
    // 2. Boot
    // 3. Resize to 2 vCPUs
    // 4. Query info, assert vcpus == 2
}

/// Create a VM with 256 MB, boot it, resize to 512 MB.
///
/// Verifies:
/// - Memory resize command succeeds
/// - `info()` reports 512 MB after resize
#[test]
#[ignore]
fn test_memory_resize() {
    // TODO: implement when running on KVM-capable host
    //
    // 1. Create VM with 256 MB
    // 2. Boot
    // 3. Resize memory to 512 MB
    // 4. Query info, assert memory == 512 MB
}

/// Create and boot a VM, hot-attach a disk image, verify via info.
///
/// Verifies:
/// - Disk attach succeeds
/// - `info()` shows the attached disk
#[test]
#[ignore]
fn test_disk_attach() {
    // TODO: implement when running on KVM-capable host
    //
    // 1. Create a temporary disk image (dd + mkfs.ext4)
    // 2. Create and boot a VM
    // 3. Call attach_disk() with the temp image
    // 4. Query info, assert disk appears in device list
}

/// After attaching a disk, detach it and verify removal.
///
/// Verifies:
/// - Disk detach succeeds
/// - `info()` no longer shows the disk
#[test]
#[ignore]
fn test_disk_detach() {
    // TODO: implement when running on KVM-capable host
    //
    // 1. Create and boot a VM
    // 2. Attach a disk
    // 3. Detach the disk
    // 4. Query info, assert disk is gone
}

/// GPU passthrough test (conditional — only runs if NVIDIA GPU + VFIO available).
///
/// Verifies:
/// - Preflight passes (VFIO bound)
/// - VM boots with the GPU device
/// - `info()` shows the device
/// - Skipped gracefully if no GPU hardware available
#[test]
#[ignore]
fn test_gpu_passthrough() {
    // TODO: implement when running on KVM-capable host with NVIDIA GPU
    //
    // 1. Check if VFIO is available; skip test if not
    // 2. Create VM with GpuMode::Passthrough
    // 3. Boot
    // 4. Query info, assert GPU device present
}

/// Replace the CH binary on disk with a fake reporting a different version.
/// Verify version_report() detects the mismatch.
///
/// Verifies:
/// - Running VM still uses old CH version
/// - version_report() shows mismatch
/// - A new VM would use the new version
#[test]
#[ignore]
fn test_binary_version_mismatch() {
    // TODO: implement when running on KVM-capable host
    //
    // 1. Create and boot a VM with CH vX
    // 2. Replace CH binary on disk with one reporting vY
    // 3. Call version_report()
    // 4. Assert report shows 1 VM on vX, disk = vY
    // 5. Assert running VM is unaffected
}

/// Attempt to create a VM with a non-existent kernel path.
///
/// Verifies:
/// - PreflightError::KernelNotFound is returned
/// - No VM artifacts are leaked
#[test]
#[ignore]
fn test_missing_kernel() {
    // TODO: implement when running on KVM-capable host
    //
    // 1. Create VM spec with kernel = "/nonexistent/vmlinux"
    // 2. Attempt to create/boot
    // 3. Assert error is KernelNotFound
    // 4. Assert no runtime directory created
}

/// Attempt to create a VM with a non-existent rootfs path.
///
/// Verifies:
/// - PreflightError::ImageNotFound is returned
/// - No VM artifacts are leaked
#[test]
#[ignore]
fn test_missing_image() {
    // TODO: implement when running on KVM-capable host
    //
    // 1. Create VM spec with image = "/nonexistent/rootfs.raw"
    // 2. Attempt to create/boot
    // 3. Assert error is ImageNotFound
    // 4. Assert no runtime directory created
}

/// Create and boot 5 VMs concurrently, verify all reach Running,
/// shut them all down, verify cleanup.
///
/// Verifies:
/// - Multiple VMs can be created concurrently
/// - All reach Running phase
/// - All can be shut down and deleted cleanly
#[test]
#[ignore]
fn test_multiple_vms_concurrent() {
    // TODO: implement when running on KVM-capable host
    //
    // 1. Create 5 VM specs with unique names
    // 2. Boot all concurrently (tokio::join! or similar)
    // 3. Assert all 5 are Running
    // 4. Shutdown all
    // 5. Assert all Stopped
    // 6. Delete all
    // 7. Assert no runtime dirs remain
}
