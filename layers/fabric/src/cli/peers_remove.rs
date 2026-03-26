use crate::control::{send_control_request, ControlRequest, ControlResponse};
use crate::{no_mesh_error, store, ui};
use anyhow::Result;

pub async fn run(name_or_key: String, skip_confirm: bool) -> Result<()> {
    let state = store::load().map_err(|_| no_mesh_error())?;

    // Check that the daemon is running
    let socket_path = store::control_socket_path();
    if !socket_path.exists() {
        anyhow::bail!("Daemon is not running. Start it first with 'syfrah fabric start'.");
    }

    // Prevent removing self on the CLI side as well
    if name_or_key == state.node_name || name_or_key == state.wg_public_key {
        anyhow::bail!("Cannot remove self. Use 'syfrah fabric leave' instead.");
    }

    // Find the peer to show a meaningful confirmation prompt
    let display_name = find_peer_display(&state, &name_or_key);

    if !skip_confirm {
        let prompt = match &display_name {
            Some((name, ipv6)) => {
                format!("Remove peer '{name}' ({ipv6})? This will disconnect it from the mesh.")
            }
            None => format!("Remove peer '{name_or_key}'? This will disconnect it from the mesh."),
        };
        if !ui::confirm(&prompt) {
            anyhow::bail!("Aborted by user.");
        }
    }

    let sp = ui::spinner("Removing peer...");

    let resp = send_control_request(
        &socket_path,
        &ControlRequest::RemovePeer {
            name_or_key: name_or_key.clone(),
        },
    )
    .await
    .map_err(|e| anyhow::anyhow!("failed to communicate with daemon: {e}"))?;

    match resp {
        ControlResponse::PeerRemoved {
            peer_name,
            announced_to,
        } => {
            ui::step_ok(&sp, &format!("Peer '{}' removed from mesh", peer_name));
            if announced_to > 0 {
                println!("    Announced removal to {announced_to} peers");
            }
            Ok(())
        }
        ControlResponse::Error { message } => {
            ui::step_fail(&sp, &message);
            anyhow::bail!("{message}");
        }
        _ => {
            ui::step_fail(&sp, "Unexpected response from daemon");
            anyhow::bail!("Unexpected response from daemon");
        }
    }
}

/// Try to find the peer by name or key in the loaded state and return
/// (name, mesh_ipv6) for a nice confirmation prompt.
fn find_peer_display(state: &store::NodeState, name_or_key: &str) -> Option<(String, String)> {
    state.peers.iter().find_map(|p| {
        if p.name == name_or_key || p.wg_public_key == name_or_key {
            Some((p.name.clone(), p.mesh_ipv6.to_string()))
        } else {
            None
        }
    })
}
