use crate::store;
use crate::ui;
use anyhow::Result;

pub async fn run() -> Result<()> {
    match store::daemon_running() {
        Some(pid) => {
            // Validate that the PID actually belongs to a syfrah process
            // to prevent killing an unrelated process if the PID was recycled
            // or the PID file was tampered with.
            if !store::is_syfrah_process(pid) {
                eprintln!(
                    "PID {pid} is not a syfrah process. Refusing to send signal. \
                     Removing stale PID file."
                );
                store::remove_pid();
                return Ok(());
            }

            let sp = ui::spinner(&format!("Stopping daemon (pid {pid})..."));
            #[cfg(unix)]
            {
                // Send SIGTERM
                unsafe { libc::kill(pid as i32, libc::SIGTERM) };
            }
            // Wait up to 10s for graceful shutdown, polling every 100ms
            for _ in 0..100 {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                if store::daemon_running().is_none() {
                    break;
                }
            }

            // Escalate to SIGKILL if SIGTERM didn't work, with retries
            if store::daemon_running().is_some() {
                let mut killed = false;
                for attempt in 0..3u8 {
                    #[cfg(unix)]
                    {
                        unsafe { libc::kill(pid as i32, libc::SIGKILL) };
                    }
                    for _ in 0..10 {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        if store::daemon_running().is_none() {
                            killed = true;
                            break;
                        }
                    }
                    if killed {
                        break;
                    }

                    match store::process_state(pid) {
                        Some('Z') => {
                            store::try_reap(pid);
                            store::remove_pid();
                            killed = true;
                            break;
                        }
                        Some('D') => {
                            ui::warn(&format!(
                                "Daemon (pid {pid}) is in uninterruptible I/O (D state), \
                                 retry {}/3...",
                                attempt + 1
                            ));
                        }
                        _ => {}
                    }
                }

                if !killed && store::daemon_running().is_some() {
                    match store::process_state(pid) {
                        Some('Z') => {
                            store::try_reap(pid);
                            store::remove_pid();
                            ui::step_ok(&sp, "Daemon reaped (was zombie).");
                        }
                        Some('D') => {
                            ui::step_fail(
                                &sp,
                                &format!(
                                    "Daemon (pid {pid}) stuck in uninterruptible I/O. \
                                     A reboot may be required."
                                ),
                            );
                        }
                        _ => {
                            ui::step_fail(
                                &sp,
                                &format!(
                                    "Daemon (pid {pid}) did not stop after 3 SIGKILL attempts. \
                                     Try 'kill -9 {pid}'."
                                ),
                            );
                        }
                    }
                } else {
                    store::remove_pid();
                    ui::step_ok(&sp, "Daemon killed (SIGKILL after 10s timeout).");
                }
            } else {
                store::remove_pid();
                ui::step_ok(&sp, "Daemon stopped.");
            }
        }
        None => {
            println!("No daemon running.");
            store::remove_pid(); // Clean up stale PID file
        }
    }
    Ok(())
}
