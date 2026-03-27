use serde::{Deserialize, Serialize};

/// Current phase in the VM lifecycle.
///
/// Every VM moves through these phases in a strict order.
/// Invalid transitions are rejected at runtime with a `TransitionError`.
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

/// Error returned when an invalid state transition is attempted.
#[derive(Debug, Clone)]
pub struct TransitionError {
    pub from: VmPhase,
    pub to: VmPhase,
}

impl std::fmt::Display for TransitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid transition from {:?} to {:?}",
            self.from, self.to
        )
    }
}

impl std::error::Error for TransitionError {}

impl VmPhase {
    /// Returns `true` if the transition from `self` to `target` is allowed.
    pub fn can_transition_to(&self, target: VmPhase) -> bool {
        matches!(
            (self, target),
            (VmPhase::Pending, VmPhase::Provisioning)
                | (VmPhase::Provisioning, VmPhase::Starting)
                | (VmPhase::Provisioning, VmPhase::Failed)
                | (VmPhase::Starting, VmPhase::Running)
                | (VmPhase::Starting, VmPhase::Failed)
                | (VmPhase::Running, VmPhase::Stopping)
                | (VmPhase::Running, VmPhase::Failed)
                | (VmPhase::Stopping, VmPhase::Stopped)
                | (VmPhase::Stopping, VmPhase::Failed)
                | (VmPhase::Stopped, VmPhase::Starting)
                | (VmPhase::Stopped, VmPhase::Deleting)
                | (VmPhase::Failed, VmPhase::Deleting)
                | (VmPhase::Deleting, VmPhase::Deleted)
                | (VmPhase::Deleting, VmPhase::Failed)
        )
    }

    /// Attempt to transition to `target`. Returns the new phase on success,
    /// or a `TransitionError` if the transition is not allowed.
    pub fn transition(self, target: VmPhase) -> Result<VmPhase, TransitionError> {
        if self.can_transition_to(target) {
            Ok(target)
        } else {
            Err(TransitionError {
                from: self,
                to: target,
            })
        }
    }

    /// Returns `true` if this phase is terminal (no further transitions possible).
    pub fn is_terminal(&self) -> bool {
        matches!(self, VmPhase::Deleted)
    }

    /// Returns `true` if the VM is in the `Failed` phase.
    pub fn is_failed(&self) -> bool {
        matches!(self, VmPhase::Failed)
    }

    /// Returns `true` if the VM is actively doing work
    /// (Provisioning, Starting, Running, or Stopping).
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            VmPhase::Provisioning | VmPhase::Starting | VmPhase::Running | VmPhase::Stopping
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_valid_transitions_succeed() {
        let cases = [
            (VmPhase::Pending, VmPhase::Provisioning),
            (VmPhase::Provisioning, VmPhase::Starting),
            (VmPhase::Provisioning, VmPhase::Failed),
            (VmPhase::Starting, VmPhase::Running),
            (VmPhase::Starting, VmPhase::Failed),
            (VmPhase::Running, VmPhase::Stopping),
            (VmPhase::Running, VmPhase::Failed),
            (VmPhase::Stopping, VmPhase::Stopped),
            (VmPhase::Stopping, VmPhase::Failed),
            (VmPhase::Stopped, VmPhase::Starting),
            (VmPhase::Stopped, VmPhase::Deleting),
            (VmPhase::Failed, VmPhase::Deleting),
            (VmPhase::Deleting, VmPhase::Deleted),
            (VmPhase::Deleting, VmPhase::Failed),
        ];
        for (from, to) in cases {
            assert!(
                from.can_transition_to(to),
                "{from:?} -> {to:?} should be allowed"
            );
            assert_eq!(from.transition(to).unwrap(), to);
        }
    }

    #[test]
    fn invalid_transitions_return_error() {
        let cases = [
            (VmPhase::Pending, VmPhase::Running),
            (VmPhase::Deleted, VmPhase::Starting),
            (VmPhase::Running, VmPhase::Provisioning),
            (VmPhase::Stopped, VmPhase::Running),
            (VmPhase::Failed, VmPhase::Running),
            (VmPhase::Deleted, VmPhase::Provisioning),
            (VmPhase::Pending, VmPhase::Stopped),
            (VmPhase::Starting, VmPhase::Stopping),
            (VmPhase::Deleted, VmPhase::Deleted),
            (VmPhase::Failed, VmPhase::Starting),
            (VmPhase::Running, VmPhase::Pending),
            (VmPhase::Running, VmPhase::Deleting),
        ];
        for (from, to) in cases {
            assert!(
                !from.can_transition_to(to),
                "{from:?} -> {to:?} should be rejected"
            );
            let err = from.transition(to).unwrap_err();
            assert_eq!(err.from, from);
            assert_eq!(err.to, to);
            assert!(err.to_string().contains("invalid transition"));
        }
    }

    #[test]
    fn is_terminal_true_only_for_deleted() {
        assert!(VmPhase::Deleted.is_terminal());
        for phase in [
            VmPhase::Pending,
            VmPhase::Provisioning,
            VmPhase::Starting,
            VmPhase::Running,
            VmPhase::Stopping,
            VmPhase::Stopped,
            VmPhase::Deleting,
            VmPhase::Failed,
        ] {
            assert!(!phase.is_terminal(), "{phase:?} should not be terminal");
        }
    }

    #[test]
    fn is_failed_true_only_for_failed() {
        assert!(VmPhase::Failed.is_failed());
        for phase in [
            VmPhase::Pending,
            VmPhase::Provisioning,
            VmPhase::Starting,
            VmPhase::Running,
            VmPhase::Stopping,
            VmPhase::Stopped,
            VmPhase::Deleting,
            VmPhase::Deleted,
        ] {
            assert!(!phase.is_failed(), "{phase:?} should not be failed");
        }
    }

    #[test]
    fn is_active_correct_phases() {
        let active = [
            VmPhase::Provisioning,
            VmPhase::Starting,
            VmPhase::Running,
            VmPhase::Stopping,
        ];
        for phase in &active {
            assert!(phase.is_active(), "{phase:?} should be active");
        }
        let inactive = [
            VmPhase::Pending,
            VmPhase::Stopped,
            VmPhase::Deleting,
            VmPhase::Deleted,
            VmPhase::Failed,
        ];
        for phase in &inactive {
            assert!(!phase.is_active(), "{phase:?} should not be active");
        }
    }

    #[test]
    fn transition_error_display_contains_from_and_to() {
        let err = VmPhase::Pending.transition(VmPhase::Running).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Pending"), "should contain from phase: {msg}");
        assert!(msg.contains("Running"), "should contain to phase: {msg}");
    }

    #[test]
    fn serde_roundtrip_all_phases() {
        let phases = [
            VmPhase::Pending,
            VmPhase::Provisioning,
            VmPhase::Starting,
            VmPhase::Running,
            VmPhase::Stopping,
            VmPhase::Stopped,
            VmPhase::Deleting,
            VmPhase::Deleted,
            VmPhase::Failed,
        ];
        for phase in phases {
            let json = serde_json::to_string(&phase).unwrap();
            let back: VmPhase = serde_json::from_str(&json).unwrap();
            assert_eq!(phase, back);
        }
    }
}
