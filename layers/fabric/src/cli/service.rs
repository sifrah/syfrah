use anyhow::{bail, Result};

#[cfg(target_os = "linux")]
use anyhow::Context;
#[cfg(target_os = "linux")]
use std::process::Command;

pub const UNIT_FILE_PATH: &str = "/etc/systemd/system/syfrah.service";

pub const UNIT_FILE_CONTENTS: &str = "\
[Unit]
Description=Syfrah mesh daemon
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/syfrah fabric start --foreground
Restart=always
RestartSec=5
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
";

pub async fn install() -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    bail!("systemd service install is only supported on Linux");

    #[cfg(target_os = "linux")]
    {
        if !has_systemctl() {
            bail!("systemctl not found. This command requires a systemd-based Linux distribution.");
        }

        std::fs::write(UNIT_FILE_PATH, UNIT_FILE_CONTENTS)
            .context("Failed to write unit file. Are you running as root?")?;
        println!("Wrote {UNIT_FILE_PATH}");

        run_systemctl(&["daemon-reload"])?;
        run_systemctl(&["enable", "syfrah"])?;

        println!("Systemd service installed and enabled.");
        println!("The daemon will start automatically on reboot.");
        println!();
        println!("To start now: systemctl start syfrah");
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
            println!("Removed {UNIT_FILE_PATH}");
        }

        run_systemctl(&["daemon-reload"])?;

        println!("Systemd service uninstalled.");
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
            println!("Systemd service is not installed.");
            println!("Run 'syfrah fabric service install' to install it.");
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
        assert!(UNIT_FILE_CONTENTS.contains("ExecStart=/usr/local/bin/syfrah fabric start --foreground"));
        assert!(UNIT_FILE_CONTENTS.contains("Restart=always"));
        assert!(UNIT_FILE_CONTENTS.contains("RestartSec=5"));
        assert!(UNIT_FILE_CONTENTS.contains("After=network-online.target"));
        assert!(UNIT_FILE_CONTENTS.contains("WantedBy=multi-user.target"));
        assert!(UNIT_FILE_CONTENTS.contains("LimitNOFILE=65535"));
    }

    #[test]
    fn unit_file_path_is_systemd_system_dir() {
        assert_eq!(UNIT_FILE_PATH, "/etc/systemd/system/syfrah.service");
    }
}
