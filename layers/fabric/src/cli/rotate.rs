use crate::audit::{self as audit_log, AuditEventType};
use crate::control::{self, ControlRequest, ControlResponse};
use crate::ui;
use crate::{no_mesh_error, store};
use anyhow::Result;
use syfrah_core::secret::MeshSecret;

pub async fn run(skip_confirm: bool) -> Result<()> {
    let state = store::load().map_err(|_| no_mesh_error())?;

    if store::daemon_running().is_some() {
        // Live rotation: the daemon is running, delegate via control socket.
        if !skip_confirm
            && !ui::confirm("Rotate mesh secret? The new secret will be broadcast to all peers.")
        {
            anyhow::bail!("aborted by user.");
        }

        let sp = ui::spinner("Rotating mesh secret (live)...");
        let socket_path = store::control_socket_path();
        let resp = control::send_control_request(&socket_path, &ControlRequest::RotateSecret)
            .await
            .map_err(|e| anyhow::anyhow!("failed to contact daemon: {e}"))?;

        match resp {
            ControlResponse::SecretRotated {
                new_secret,
                new_ipv6,
                peers_notified,
                peers_failed,
            } => {
                ui::step_ok(&sp, "Secret rotated (live)");
                ui::info_line("New secret", &new_secret);
                ui::info_line("New IPv6", &new_ipv6);
                ui::info_line("Peers notified", &peers_notified.to_string());
                if peers_failed > 0 {
                    ui::info_line("Peers failed", &peers_failed.to_string());
                    println!();
                    println!("Some peers could not be reached. They will need to rejoin.");
                }
            }
            ControlResponse::Error { message } => {
                ui::step_fail(&sp, "Secret rotation failed");
                anyhow::bail!("{message}");
            }
            other => {
                ui::step_fail(&sp, "Secret rotation failed");
                anyhow::bail!("unexpected response from daemon: {other:?}");
            }
        }
    } else {
        // Offline rotation: daemon is stopped, update state directly.
        if !skip_confirm && !ui::confirm("Rotate mesh secret? All peers must rejoin afterwards.") {
            anyhow::bail!("aborted by user.");
        }

        let sp = ui::spinner("Rotating mesh secret...");
        let new_secret = MeshSecret::generate();
        let new_prefix = crate::daemon::derive_prefix_from_secret(&new_secret);
        let new_ipv6 = syfrah_core::addressing::derive_node_address(
            &new_prefix,
            wireguard_control::Key::from_base64(&state.wg_public_key)
                .map_err(|_| anyhow::anyhow!("bad WG key"))?
                .as_bytes(),
        );

        let mut state = state;
        state.mesh_secret = new_secret.to_string();
        state.mesh_prefix = new_prefix;
        state.mesh_ipv6 = new_ipv6;
        state.peers.clear();
        store::save(&state)?;
        audit_log::emit(AuditEventType::SecretRotated, None, None, None);

        ui::step_ok(&sp, "Secret rotated");
        ui::info_line("New secret", &new_secret.to_string());
        ui::info_line("New IPv6", &new_ipv6.to_string());
        println!();
        println!("All peers must rejoin with the new secret.");
        println!("Restart this node with 'syfrah fabric start'.");
    }
    Ok(())
}
