use crate::types::VmId;

/// Top-level error type for the compute layer.
///
/// Forge pattern-matches on the variant to decide how to handle each error:
/// - User error (Config, Preflight) -> return to caller
/// - Infra error (Client, Process) -> retry or alert
/// - Transient error (Concurrency) -> retry with backoff
/// - Bug (Transition) -> log and escalate
#[derive(Debug, thiserror::Error)]
pub enum ComputeError {
    #[error("preflight check failed: {0}")]
    Preflight(#[from] PreflightError),

    #[error("configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("cloud hypervisor client error: {0}")]
    Client(#[from] ClientError),

    #[error("process management error: {0}")]
    Process(#[from] ProcessError),

    #[error("invalid state transition: {0}")]
    Transition(#[from] TransitionError),

    #[error("concurrency error: {0}")]
    Concurrency(#[from] ConcurrencyError),
}

/// Precondition not met before spawning a VM.
///
/// The preflight validator collects all failures in a single pass and returns
/// them together as `Vec<PreflightError>`.
#[derive(Debug, thiserror::Error)]
pub enum PreflightError {
    #[error("cloud-hypervisor binary not found")]
    ChBinaryNotFound,

    #[error("/dev/kvm is not available")]
    KvmNotAvailable,

    #[error("kernel image not found")]
    KernelNotFound,

    #[error("disk image not found")]
    ImageNotFound,

    #[error("TAP device not found")]
    TapDeviceNotFound,

    #[error("VFIO device not bound for PCI address {bdf}")]
    VfioNotBound { bdf: String },

    #[error("cgroup v2 is not available")]
    CgroupV2NotAvailable,

    #[error("socket path already occupied: {path}")]
    SocketPathOccupied { path: String },

    #[error("insufficient {resource}: available {available}, required {required}")]
    InsufficientResources {
        resource: String,
        available: String,
        required: String,
    },
}

/// Invalid or unresolvable VM spec.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("invalid vcpu count: {value}")]
    InvalidVcpuCount { value: u32 },

    #[error("invalid memory size: {value} MB")]
    InvalidMemory { value: u32 },

    #[error("unknown image: {name}")]
    UnknownImage { name: String },

    #[error("invalid PCI BDF address: {bdf}")]
    InvalidBdf { bdf: String },

    #[error("kernel path is required but not provided")]
    MissingKernel,

    #[error("conflicting settings: {detail}")]
    ConflictingSettings { detail: String },
}

/// Cloud Hypervisor REST API call failure.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("API socket not found: {path}")]
    SocketNotFound { path: String },

    #[error("connection refused")]
    ConnectionRefused,

    #[error("timeout during {operation}")]
    Timeout { operation: String },

    #[error("unexpected HTTP status {status}: {body}")]
    UnexpectedStatus { status: u16, body: String },

    #[error("invalid response: {detail}")]
    InvalidResponse { detail: String },
}

/// OS-level process management failure.
#[derive(Debug, thiserror::Error)]
pub enum ProcessError {
    #[error("failed to spawn process: {reason}")]
    SpawnFailed { reason: String },

    #[error("PID {pid} not found")]
    PidNotFound { pid: u32 },

    #[error("cgroup error: {detail}")]
    CgroupError { detail: String },

    #[error("failed to send signal {signal} to PID {pid}")]
    SignalFailed { signal: String, pid: u32 },
}

/// Invalid state machine transition.
#[derive(Debug, thiserror::Error)]
#[error("cannot transition from {from} to {to}")]
pub struct TransitionError {
    pub from: String,
    pub to: String,
}

/// Operation blocked by a concurrent operation on the same VM.
#[derive(Debug, thiserror::Error)]
#[error("operation on VM {vm_id} blocked by {blocked_by}")]
pub struct ConcurrencyError {
    pub vm_id: VmId,
    pub blocked_by: String,
}
