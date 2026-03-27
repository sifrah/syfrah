use serde::{Deserialize, Serialize};

/// Current phase in the VM lifecycle.
///
/// TODO: Full state machine with transition validation will be added
/// when the phase module is fully implemented. This is a minimal definition
/// so dependent types can compile.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmPhase {
    Pending,
    Provisioning,
    Starting,
    Running,
    Stopping,
    Stopped,
    Deleting,
    Deleted,
    Failed,
}
