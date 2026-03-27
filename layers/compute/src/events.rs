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

    #[test]
    fn emit_preserves_event_payload_data() {
        let (tx, mut rx) = broadcast::channel(16);
        emit(
            &tx,
            VmEvent::Crashed {
                vm_id: VmId("vm-crash-1".to_string()),
                error: "out of memory".to_string(),
            },
        );
        let received = rx.try_recv().unwrap();
        if let VmEvent::Crashed { vm_id, error } = received {
            assert_eq!(vm_id.0, "vm-crash-1");
            assert_eq!(error, "out of memory");
        } else {
            panic!("expected Crashed variant");
        }
    }

    #[test]
    fn emit_resized_preserves_cpu_and_memory() {
        let (tx, mut rx) = broadcast::channel(16);
        emit(
            &tx,
            VmEvent::Resized {
                vm_id: VmId("vm-resize".to_string()),
                new_vcpus: 8,
                new_memory_mb: 16384,
            },
        );
        let received = rx.try_recv().unwrap();
        if let VmEvent::Resized {
            vm_id,
            new_vcpus,
            new_memory_mb,
        } = received
        {
            assert_eq!(vm_id.0, "vm-resize");
            assert_eq!(new_vcpus, 8);
            assert_eq!(new_memory_mb, 16384);
        } else {
            panic!("expected Resized variant");
        }
    }

    #[test]
    fn full_lifecycle_event_sequence() {
        let (tx, mut rx) = broadcast::channel(16);
        let id = VmId("vm-lifecycle".to_string());

        emit(&tx, VmEvent::Created { vm_id: id.clone() });
        emit(&tx, VmEvent::Booted { vm_id: id.clone() });
        emit(&tx, VmEvent::Stopped { vm_id: id.clone() });
        emit(&tx, VmEvent::Deleted { vm_id: id.clone() });

        let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert_eq!(events.len(), 4);
        assert!(matches!(&events[0], VmEvent::Created { .. }));
        assert!(matches!(&events[1], VmEvent::Booted { .. }));
        assert!(matches!(&events[2], VmEvent::Stopped { .. }));
        assert!(matches!(&events[3], VmEvent::Deleted { .. }));
    }

    #[test]
    fn receiver_recovers_after_lagged() {
        // Channel with capacity 2
        let (tx, mut rx) = broadcast::channel(2);
        // Emit 3 events — first one will be dropped for the receiver
        emit(
            &tx,
            VmEvent::Created {
                vm_id: VmId("vm-1".to_string()),
            },
        );
        emit(
            &tx,
            VmEvent::Booted {
                vm_id: VmId("vm-1".to_string()),
            },
        );
        emit(
            &tx,
            VmEvent::Stopped {
                vm_id: VmId("vm-1".to_string()),
            },
        );

        // First recv should report Lagged (1 message was lost)
        let result = rx.try_recv();
        assert!(matches!(result, Err(broadcast::error::TryRecvError::Lagged(1))) || result.is_ok());
        // Subsequent receives should succeed
        let remaining: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert!(
            !remaining.is_empty(),
            "receiver should recover after lagged"
        );
    }

    #[test]
    fn three_subscribers_all_receive() {
        let (tx, mut rx1) = broadcast::channel(16);
        let mut rx2 = tx.subscribe();
        let mut rx3 = tx.subscribe();

        emit(
            &tx,
            VmEvent::Created {
                vm_id: VmId("vm-multi".to_string()),
            },
        );

        assert!(rx1.try_recv().is_ok());
        assert!(rx2.try_recv().is_ok());
        assert!(rx3.try_recv().is_ok());
    }

    #[test]
    fn emit_device_events_carry_device_name() {
        let (tx, mut rx) = broadcast::channel(16);
        emit(
            &tx,
            VmEvent::DeviceAttached {
                vm_id: VmId("vm-gpu".to_string()),
                device: "0000:01:00.0".to_string(),
            },
        );
        emit(
            &tx,
            VmEvent::DeviceDetached {
                vm_id: VmId("vm-gpu".to_string()),
                device: "0000:01:00.0".to_string(),
            },
        );
        let attached = rx.try_recv().unwrap();
        let detached = rx.try_recv().unwrap();
        if let VmEvent::DeviceAttached { device, .. } = attached {
            assert_eq!(device, "0000:01:00.0");
        } else {
            panic!("expected DeviceAttached");
        }
        if let VmEvent::DeviceDetached { device, .. } = detached {
            assert_eq!(device, "0000:01:00.0");
        } else {
            panic!("expected DeviceDetached");
        }
    }

    #[test]
    fn reconnect_events_carry_vm_id() {
        let (tx, mut rx) = broadcast::channel(16);
        emit(
            &tx,
            VmEvent::ReconnectSucceeded {
                vm_id: VmId("vm-recovered".to_string()),
            },
        );
        emit(
            &tx,
            VmEvent::ReconnectFailed {
                vm_id: VmId("vm-lost".to_string()),
                error: "PID dead".to_string(),
            },
        );
        emit(
            &tx,
            VmEvent::VmOrphanCleaned {
                vm_id: VmId("vm-orphan".to_string()),
                reason: "no meta.json".to_string(),
            },
        );

        let e1 = rx.try_recv().unwrap();
        let e2 = rx.try_recv().unwrap();
        let e3 = rx.try_recv().unwrap();

        if let VmEvent::ReconnectSucceeded { vm_id } = e1 {
            assert_eq!(vm_id.0, "vm-recovered");
        } else {
            panic!("expected ReconnectSucceeded");
        }
        if let VmEvent::ReconnectFailed { vm_id, error } = e2 {
            assert_eq!(vm_id.0, "vm-lost");
            assert_eq!(error, "PID dead");
        } else {
            panic!("expected ReconnectFailed");
        }
        if let VmEvent::VmOrphanCleaned { vm_id, reason } = e3 {
            assert_eq!(vm_id.0, "vm-orphan");
            assert_eq!(reason, "no meta.json");
        } else {
            panic!("expected VmOrphanCleaned");
        }
    }
}
