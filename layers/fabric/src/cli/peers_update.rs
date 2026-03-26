use std::net::SocketAddr;

use crate::control::{send_control_request, ControlRequest, ControlResponse};
use crate::{no_mesh_error, store, ui};
use anyhow::Result;

pub async fn run(name_or_key: String, endpoint: SocketAddr) -> Result<()> {
    let state = store::load().map_err(|_| no_mesh_error())?;

    // Check that the daemon is running
    let socket_path = store::control_socket_path();
    if !socket_path.exists() {
        anyhow::bail!("Daemon is not running. Start it first with 'syfrah fabric start'.");
    }

    // Prevent updating self
    if name_or_key == state.node_name || name_or_key == state.wg_public_key {
        anyhow::bail!("Cannot update self endpoint via peers update. Use 'syfrah fabric leave' and rejoin instead.");
    }

    let sp = ui::spinner("Updating peer endpoint...");

    let resp = send_control_request(
        &socket_path,
        &ControlRequest::UpdatePeerEndpoint {
            name_or_key: name_or_key.clone(),
            endpoint,
        },
    )
    .await
    .map_err(|e| anyhow::anyhow!("failed to communicate with daemon: {e}"))?;

    match resp {
        ControlResponse::PeerEndpointUpdated {
            peer_name,
            old_endpoint,
            new_endpoint,
        } => {
            ui::step_ok(
                &sp,
                &format!(
                    "Updated endpoint for '{}': {} \u{2192} {}",
                    peer_name, old_endpoint, new_endpoint
                ),
            );
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
