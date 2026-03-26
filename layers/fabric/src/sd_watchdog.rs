//! Systemd watchdog and readiness notification helpers.
//!
//! When the daemon runs under systemd with `Type=notify` and `WatchdogSec`,
//! these functions send the appropriate `sd_notify` messages. When not running
//! under systemd (no `NOTIFY_SOCKET` in the environment), all calls are
//! graceful no-ops.

use tracing::{debug, warn};

/// Returns `true` if the process appears to be running under a systemd
/// service manager that expects `sd_notify` messages (i.e. `NOTIFY_SOCKET`
/// is set in the environment).
pub fn is_active() -> bool {
    std::env::var_os("NOTIFY_SOCKET").is_some()
}

/// Notify systemd that the daemon is ready (`READY=1`).
///
/// Should be called once, after the WireGuard interface is up and the
/// control socket is listening.
pub fn notify_ready() {
    if !is_active() {
        debug!("sd_notify: not running under systemd, skipping READY=1");
        return;
    }
    debug!("sd_notify: sending READY=1");
    if let Err(e) = sd_notify::notify(false, &[sd_notify::NotifyState::Ready]) {
        warn!(error = %e, "sd_notify: failed to send READY=1");
    }
}

/// Ping the systemd watchdog (`WATCHDOG=1`).
///
/// Must be called at an interval shorter than `WatchdogSec` configured in
/// the unit file. The health-check loop calls this every iteration.
pub fn notify_watchdog() {
    if !is_active() {
        return;
    }
    debug!("sd_notify: sending WATCHDOG=1");
    if let Err(e) = sd_notify::notify(false, &[sd_notify::NotifyState::Watchdog]) {
        warn!(error = %e, "sd_notify: failed to send WATCHDOG=1");
    }
}

/// Notify systemd that the daemon is stopping (`STOPPING=1`).
pub fn notify_stopping() {
    if !is_active() {
        return;
    }
    debug!("sd_notify: sending STOPPING=1");
    if let Err(e) = sd_notify::notify(false, &[sd_notify::NotifyState::Stopping]) {
        warn!(error = %e, "sd_notify: failed to send STOPPING=1");
    }
}

/// Notify systemd of the daemon's current status line.
pub fn notify_status(msg: &str) {
    if !is_active() {
        return;
    }
    debug!(status = msg, "sd_notify: sending STATUS");
    if let Err(e) = sd_notify::notify(false, &[sd_notify::NotifyState::Status(msg)]) {
        warn!(error = %e, "sd_notify: failed to send STATUS");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_active_returns_false_without_notify_socket() {
        // In test environment NOTIFY_SOCKET is not set, so sd_notify is inactive.
        std::env::remove_var("NOTIFY_SOCKET");
        assert!(!is_active());
    }

    #[test]
    fn notify_ready_is_noop_without_systemd() {
        std::env::remove_var("NOTIFY_SOCKET");
        // Should not panic or error when not running under systemd.
        notify_ready();
    }

    #[test]
    fn notify_watchdog_is_noop_without_systemd() {
        std::env::remove_var("NOTIFY_SOCKET");
        notify_watchdog();
    }

    #[test]
    fn notify_stopping_is_noop_without_systemd() {
        std::env::remove_var("NOTIFY_SOCKET");
        notify_stopping();
    }

    #[test]
    fn notify_status_is_noop_without_systemd() {
        std::env::remove_var("NOTIFY_SOCKET");
        notify_status("test status");
    }
}
