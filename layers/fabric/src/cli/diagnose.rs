use crate::{store, ui, wg};
use anyhow::Result;
use syfrah_state::LayerDb;

pub async fn run() -> Result<()> {
    ui::heading("Syfrah Fabric Diagnostics");
    println!();

    let mut pass_count = 0u32;
    let mut fail_count = 0u32;

    let mut check = |name: &str, result: bool, detail: &str| {
        if result {
            ui::check_pass(name);
            pass_count += 1;
        } else {
            ui::check_fail(name, detail);
            fail_count += 1;
        }
    };

    // -- State store --
    println!("State store");
    let state_exists = store::exists();
    check(
        "Mesh state exists",
        state_exists,
        "run 'syfrah fabric init' or 'syfrah fabric join'",
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
        check("state.json is valid JSON", json_ok, "file may be corrupted");
    }

    let redb_ok = LayerDb::layer_exists("fabric") && LayerDb::open("fabric").is_ok();
    check(
        "redb database is readable",
        redb_ok || !LayerDb::layer_exists("fabric"),
        "fabric.redb may be corrupted",
    );

    let state = store::load();
    if let Ok(ref s) = state {
        check(
            &format!("Loaded {} peers from state", s.peers.len()),
            true,
            "",
        );
    }
    println!();

    // -- Daemon --
    println!("Daemon");
    let pid = store::daemon_running();
    check(
        "Daemon process",
        pid.is_some(),
        "daemon is not running — start with 'syfrah fabric start'",
    );

    let socket_path = store::control_socket_path();
    let socket_exists = socket_path.exists();
    check(
        "Control socket exists",
        socket_exists,
        &format!("missing: {}", socket_path.display()),
    );

    let log_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".syfrah")
        .join("syfrah.log");
    if log_path.exists() {
        let log_size = std::fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);
        check(&format!("Log file ({} bytes)", log_size), true, "");
    } else {
        check("Log file", false, "~/.syfrah/syfrah.log not found");
    }
    println!();

    // -- WireGuard --
    println!("WireGuard");
    match wg::interface_summary() {
        Ok(summary) => {
            check("Interface syfrah0 is up", true, "");
            check(
                &format!(
                    "{} WG peers configured, {} with handshake",
                    summary.peer_count,
                    summary
                        .peers
                        .iter()
                        .filter(|p| p.last_handshake.is_some())
                        .count()
                ),
                true,
                "",
            );

            // Check consistency: stored peers vs WG peers
            if let Ok(ref s) = state {
                let stored_count = s.peers.len();
                let wg_count = summary.peer_count;
                let consistent = stored_count == wg_count;
                check(
                    &format!("Store/WG consistency ({stored_count} stored, {wg_count} in WG)"),
                    consistent,
                    "mismatch — reconciliation may fix this",
                );
            }
        }
        Err(e) => {
            check("Interface syfrah0", false, &format!("not found: {e}"));
        }
    }
    println!();

    // -- Summary --
    let total = pass_count + fail_count;
    if fail_count == 0 {
        ui::success(&format!("{pass_count}/{total} checks passed. Fabric is healthy."));
    } else {
        ui::warn(&format!("{fail_count}/{total} checks failed."));
    }

    Ok(())
}
