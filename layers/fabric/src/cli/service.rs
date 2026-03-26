use anyhow::{bail, Context, Result};

use crate::ui;
#[cfg(target_os = "linux")]
use std::process::Command;

pub const UNIT_FILE_PATH: &str = "/etc/systemd/system/syfrah.service";

pub fn unit_file_contents() -> Result<String> {
    let exe = std::env::current_exe().context("Failed to determine current executable path")?;
    let exe_path = exe
        .to_str()
        .context("Executable path contains invalid UTF-8")?;
    Ok(format!(
        "\
[Unit]
Description=Syfrah mesh daemon
After=network-online.target
Wants=network-online.target

[Service]
Type=notify
ExecStart={exe_path} fabric start --foreground
ExecStartPost={exe_path} fabric diagnose --json
Restart=always
RestartSec=5
WatchdogSec=60
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
"
    ))
}

pub async fn install() -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    bail!("systemd service install is only supported on Linux");

    #[cfg(target_os = "linux")]
    {
        if !has_systemctl() {
            bail!("systemctl not found. This command requires a systemd-based Linux distribution.");
        }

        if std::path::Path::new(UNIT_FILE_PATH).exists() {
            println!("Systemd service is already installed at {UNIT_FILE_PATH}.");
            if !ui::confirm("Overwrite the existing unit file?") {
                bail!("Aborted — existing unit file was not modified.");
            }
        }

        let contents = unit_file_contents()?;
        std::fs::write(UNIT_FILE_PATH, &contents)
            .context("Failed to write unit file. Are you running as root?")?;
        ui::success(&format!("Wrote {UNIT_FILE_PATH}"));

        run_systemctl(&["daemon-reload"])?;
        run_systemctl(&["enable", "syfrah"])?;

        ui::success("Systemd service installed and enabled.");
        ui::info_line("Type", "notify (daemon signals readiness via sd_notify)");
        ui::info_line("Watchdog", "WatchdogSec=60 (health check pings every 30s)");
        ui::info_line("Note", "The daemon will start automatically on reboot.");
        ui::info_line("Start now", "systemctl start syfrah");
        Ok(())
    }
}

pub async fn uninstall() -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    bail!("systemd service uninstall is only supported on Linux");

    #[cfg(target_os = "linux")]
    {
        if !has_systemctl() {
            bail!("systemctl not found. This command requires a systemd-based Linux distribution.");
        }

        // Stop if running, ignore errors (may not be running)
        let _ = run_systemctl(&["stop", "syfrah"]);
        let _ = run_systemctl(&["disable", "syfrah"]);

        if std::path::Path::new(UNIT_FILE_PATH).exists() {
            std::fs::remove_file(UNIT_FILE_PATH)
                .context("Failed to remove unit file. Are you running as root?")?;
            ui::success(&format!("Removed {UNIT_FILE_PATH}"));
        }

        run_systemctl(&["daemon-reload"])?;

        ui::success("Systemd service uninstalled.");
        Ok(())
    }
}

pub async fn status() -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    bail!("systemd service status is only supported on Linux");

    #[cfg(target_os = "linux")]
    {
        if !has_systemctl() {
            bail!("systemctl not found. This command requires a systemd-based Linux distribution.");
        }

        if !std::path::Path::new(UNIT_FILE_PATH).exists() {
            ui::warn("Systemd service is not installed.");
            ui::info_line("Install", "syfrah fabric service install");
            return Ok(());
        }

        let output = Command::new("systemctl")
            .args(["status", "syfrah"])
            .output()
            .context("Failed to run systemctl")?;

        // systemctl status returns exit code 3 when service is stopped, which is fine
        print!("{}", String::from_utf8_lossy(&output.stdout));
        if !output.stderr.is_empty() {
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
        }

        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn has_systemctl() -> bool {
    Command::new("systemctl")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn run_systemctl(args: &[&str]) -> Result<()> {
    let output = Command::new("systemctl")
        .args(args)
        .output()
        .with_context(|| format!("Failed to run: systemctl {}", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("systemctl {} failed: {}", args.join(" "), stderr.trim());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_file_contains_required_directives() {
        let contents = unit_file_contents().expect("should resolve current_exe in tests");
        assert!(contents.contains("ExecStart="));
        assert!(contents.contains("fabric start --foreground"));
        assert!(contents.contains("Restart=always"));
        assert!(contents.contains("RestartSec=5"));
        assert!(contents.contains("After=network-online.target"));
        assert!(contents.contains("WantedBy=multi-user.target"));
        assert!(contents.contains("LimitNOFILE=65535"));
        assert!(contents.contains("Type=notify"));
        assert!(contents.contains("WatchdogSec=60"));
        assert!(contents.contains("ExecStartPost="));
        assert!(contents.contains("fabric diagnose --json"));
    }

    #[test]
    fn unit_file_path_is_systemd_system_dir() {
        assert_eq!(UNIT_FILE_PATH, "/etc/systemd/system/syfrah.service");
    }
}
