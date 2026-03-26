use crate::{audit, config, sd_watchdog, store, ui, wg};
use anyhow::Result;
use serde::Serialize;
use syfrah_state::LayerDb;

#[derive(Serialize)]
struct DiagnoseCheck {
    name: String,
    passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

#[derive(Serialize)]
struct DiagnoseOutput {
    checks: Vec<DiagnoseCheck>,
    passed: u32,
    failed: u32,
    total: u32,
}

pub async fn run(json: bool) -> Result<()> {
    let tuning = config::load_tuning().unwrap_or_default();
    wg::set_interface_name(&tuning.interface_name);

    let mut checks: Vec<DiagnoseCheck> = Vec::new();
    let mut pass_count = 0u32;
    let mut fail_count = 0u32;

    macro_rules! check {
        ($name:expr, $result:expr, $detail:expr) => {{
            let name: String = $name.into();
            let result: bool = $result;
            let detail: &str = $detail;
            if result {
                pass_count += 1;
            } else {
                fail_count += 1;
            }
            if !json {
                if result {
                    ui::check_pass(&name);
                } else {
                    ui::check_fail(&name, detail);
                }
            }
            checks.push(DiagnoseCheck {
                name,
                passed: result,
                detail: if detail.is_empty() {
                    None
                } else {
                    Some(detail.to_string())
                },
            });
        }};
    }

    if !json {
        ui::heading("Syfrah Fabric Diagnostics");
        println!();
    }

    // -- State store --
    if !json {
        ui::heading("State store");
    }
    let state_exists = store::exists();
    check!(
        "Mesh state exists",
        state_exists,
        "run 'syfrah fabric init' or 'syfrah fabric join'"
    );

    let state_file = dirs::home_dir()
        .unwrap_or_default()
        .join(".syfrah")
        .join("state.json");
    let json_ok = if state_file.exists() {
        std::fs::read_to_string(&state_file)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .is_some()
    } else {
        false
    };
    if state_file.exists() {
        check!("state.json is valid JSON", json_ok, "file may be corrupted");
    }

    let redb_ok = LayerDb::layer_exists("fabric") && LayerDb::open("fabric").is_ok();
    check!(
        "redb database is readable",
        redb_ok || !LayerDb::layer_exists("fabric"),
        "fabric.redb may be corrupted"
    );

    let state = store::load();
    if let Ok(ref s) = state {
        check!(
            format!("Loaded {} peers from state", s.peers.len()),
            true,
            ""
        );
    }
    if !json {
        println!();
    }

    // -- Daemon --
    if !json {
        ui::heading("Daemon");
    }
    let pid = store::daemon_running();
    check!(
        "Daemon process",
        pid.is_some(),
        "daemon is not running — start with 'syfrah fabric start'"
    );

    let socket_path = store::control_socket_path();
    let socket_exists = socket_path.exists();
    check!(
        "Control socket exists",
        socket_exists,
        &format!("missing: {}", socket_path.display())
    );

    let audit_path = audit::audit_log_path();
    let audit_ok = if audit_path.exists() {
        let writable = std::fs::OpenOptions::new()
            .append(true)
            .open(&audit_path)
            .is_ok();
        let size = std::fs::metadata(&audit_path).map(|m| m.len()).unwrap_or(0);
        check!(
            format!("Audit log exists ({size} bytes)"),
            writable,
            "audit log is not writable"
        );
        writable
    } else {
        // Not an error if the log doesn't exist yet (first run).
        check!("Audit log", true, "");
        true
    };
    let _ = audit_ok;

    let log_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".syfrah")
        .join("syfrah.log");
    if log_path.exists() {
        let log_size = std::fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);
        check!(format!("Log file ({} bytes)", log_size), true, "");
    } else {
        check!("Log file", false, "~/.syfrah/syfrah.log not found");
    }
    if !json {
        println!();
    }

    // -- WireGuard --
    if !json {
        ui::heading("WireGuard");
    }
    match wg::interface_summary() {
        Ok(summary) => {
            check!(
                format!("Interface {} is up", wg::interface_name()),
                true,
                ""
            );
            check!(
                format!(
                    "{} WG peers configured, {} with handshake",
                    summary.peer_count,
                    summary
                        .peers
                        .iter()
                        .filter(|p| p.last_handshake.is_some())
                        .count()
                ),
                true,
                ""
            );

            // Check consistency: stored peers vs WG peers
            if let Ok(ref s) = state {
                let stored_count = s.peers.len();
                let wg_count = summary.peer_count;
                let consistent = stored_count == wg_count;
                check!(
                    format!("Store/WG consistency ({stored_count} stored, {wg_count} in WG)"),
                    consistent,
                    "mismatch — reconciliation may fix this"
                );
            }
        }
        Err(e) => {
            check!(
                format!("Interface {}", wg::interface_name()),
                false,
                &format!("not found: {e}")
            );
        }
    }

    // -- Systemd integration --
    if !json {
        println!();
        ui::heading("Systemd");
    }
    let unit_installed = std::path::Path::new(crate::cli::service::UNIT_FILE_PATH).exists();
    check!(
        "Unit file installed",
        unit_installed,
        "run 'syfrah fabric service install'"
    );
    let sd_active = sd_watchdog::is_active();
    check!(
        "Systemd notify socket",
        sd_active || !unit_installed,
        "NOTIFY_SOCKET not set — daemon may not be running under systemd"
    );
    if unit_installed {
        if let Ok(contents) = std::fs::read_to_string(crate::cli::service::UNIT_FILE_PATH) {
            let has_notify = contents.contains("Type=notify");
            check!(
                "Unit file has Type=notify",
                has_notify,
                "reinstall with 'syfrah fabric service install'"
            );
            let has_watchdog = contents.contains("WatchdogSec=");
            check!(
                "Unit file has WatchdogSec",
                has_watchdog,
                "reinstall with 'syfrah fabric service install'"
            );
        }
    }

    if json {
        let total = pass_count + fail_count;
        let output = DiagnoseOutput {
            checks,
            passed: pass_count,
            failed: fail_count,
            total,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!();

        // -- Summary --
        let total = pass_count + fail_count;
        if fail_count == 0 {
            ui::success(&format!(
                "{pass_count}/{total} checks passed. Fabric is healthy."
            ));
        } else {
            ui::warn(&format!("{fail_count}/{total} checks failed."));
        }
    }

    Ok(())
}
