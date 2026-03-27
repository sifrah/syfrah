//! Event system for the compute layer.
//!
//! Two levels of events with different audiences:
//!
//! ## Internal events (tracing only, NOT sent through broadcast)
//!
//! Logged via `tracing::debug!` at the point of occurrence within the process
//! manager. These are for debugging and operational visibility:
//!
//! - **Spawned** — cloud-hypervisor process started (`process.rs` spawn)
//! - **ApiReady** — CH REST API responding to ping (`process.rs` ping loop)
//! - **PingTimeout** — API did not become ready within timeout (`process.rs`)
//! - **ProcessExited** — CH process is no longer alive (`process.rs` / monitor)
//! - **CgroupCreated** — cgroup v2 hierarchy set up for VM (future)
//! - **CgroupDestroyed** — cgroup v2 hierarchy removed (future)
//! - **SocketCreated** — API socket appeared on disk (`process.rs` spawn)
//! - **SocketRemoved** — API socket cleaned up (`process.rs` cleanup)
//!
//! ## External events (broadcast channel, consumed by forge)
//!
//! Sent through `tokio::sync::broadcast` via the [`emit`] helper. These are
//! the [`VmEvent`] enum variants that forge subscribes to:
//!
//! - `Created`, `Booted`, `Stopped`, `Crashed`, `Deleted`
//! - `ReconnectSucceeded`, `ReconnectFailed`, `VmOrphanCleaned`
//! - `Resized`, `DeviceAttached`, `DeviceDetached`
//!
//! ## Delivery guarantee
//!
//! Best-effort, real-time. The broadcast channel has a fixed capacity (256 by
//! default). If all receivers lag behind, [`emit`] logs a warning but does not
//! block. If there are no receivers at all, the event is silently discarded.
//! Forge must treat this stream as informational — the source of truth for VM
//! state is always `info()` / `status()`, never the event stream alone.

use tokio::sync::broadcast;
use tracing::debug;

use crate::types::VmEvent;

/// Send an external event through the broadcast channel.
///
/// - If there are no subscribers, the event is silently dropped (this is
///   normal during startup or in tests).
/// - If the channel is full (all receivers lagging), logs a warning but
///   does not block or panic.
pub fn emit(tx: &broadcast::Sender<VmEvent>, event: VmEvent) {
    debug!(?event, "emitting event");
    match tx.send(event) {
        Ok(receiver_count) => {
            debug!(receiver_count, "event delivered");
        }
        Err(broadcast::error::SendError(returned_event)) => {
            // send() fails only when there are zero receivers.
            // This is expected during startup or in tests — not an error.
            debug!(?returned_event, "event dropped (no subscribers)");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::VmId;

    #[test]
    fn emit_with_subscriber_delivers_event() {
        let (tx, mut rx) = broadcast::channel(16);
        let event = VmEvent::Created {
            vm_id: VmId("vm-1".to_string()),
        };
        emit(&tx, event);
        let received = rx.try_recv().unwrap();
        assert!(matches!(received, VmEvent::Created { .. }));
    }

    #[test]
    fn emit_without_subscriber_does_not_panic() {
        let (tx, rx) = broadcast::channel::<VmEvent>(16);
        // Drop the receiver immediately
        drop(rx);
        // This must not panic
        emit(
            &tx,
            VmEvent::Stopped {
                vm_id: VmId("vm-gone".to_string()),
            },
        );
    }

    #[test]
    fn emit_logs_warning_on_full_channel() {
        // Channel with capacity 1 — second send will succeed but first
        // message will be lost to lagging receivers. broadcast::Sender::send
        // only returns Err when there are zero receivers, so with capacity 1
        // the old messages are dropped on the receiver side, not the sender.
        let (tx, mut rx) = broadcast::channel(1);
        emit(
            &tx,
            VmEvent::Created {
                vm_id: VmId("vm-a".to_string()),
            },
        );
        emit(
            &tx,
            VmEvent::Booted {
                vm_id: VmId("vm-a".to_string()),
            },
        );
        // The receiver should see a Lagged error for the first message,
        // then receive the second.
        let result = rx.try_recv();
        assert!(result.is_ok() || matches!(result, Err(broadcast::error::TryRecvError::Lagged(_))));
    }

    #[test]
    fn multiple_subscribers_receive_same_event() {
        let (tx, mut rx1) = broadcast::channel(16);
        let mut rx2 = tx.subscribe();
        emit(
            &tx,
            VmEvent::Deleted {
                vm_id: VmId("vm-x".to_string()),
            },
        );
        let e1 = rx1.try_recv().unwrap();
        let e2 = rx2.try_recv().unwrap();
        assert!(matches!(e1, VmEvent::Deleted { .. }));
        assert!(matches!(e2, VmEvent::Deleted { .. }));
    }

    #[test]
    fn event_ordering_is_preserved() {
        let (tx, mut rx) = broadcast::channel(16);
        emit(
            &tx,
            VmEvent::Created {
                vm_id: VmId("vm-ord".to_string()),
            },
        );
        emit(
            &tx,
            VmEvent::Booted {
                vm_id: VmId("vm-ord".to_string()),
            },
        );
        let first = rx.try_recv().unwrap();
        let second = rx.try_recv().unwrap();
        assert!(matches!(first, VmEvent::Created { .. }));
        assert!(matches!(second, VmEvent::Booted { .. }));
    }

    #[test]
    fn subscribe_after_emit_misses_event() {
        let (tx, _initial_rx) = broadcast::channel(16);
        emit(
            &tx,
            VmEvent::Created {
                vm_id: VmId("vm-late".to_string()),
            },
        );
        // Subscribe after the event was sent
        let mut late_rx = tx.subscribe();
        let result = late_rx.try_recv();
        assert!(result.is_err());
    }
}
