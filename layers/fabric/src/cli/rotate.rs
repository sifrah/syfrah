use crate::ui;
use crate::{no_mesh_error, store};
use anyhow::Result;
use syfrah_core::secret::MeshSecret;

pub async fn run() -> Result<()> {
    let mut state = store::load().map_err(|_| no_mesh_error())?;

    if store::daemon_running().is_some() {
        anyhow::bail!("daemon is running. Stop it first with 'syfrah fabric stop'.");
    }

    if !ui::confirm("Rotate mesh secret? All peers must rejoin afterwards.") {
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

    state.mesh_secret = new_secret.to_string();
    state.mesh_prefix = new_prefix;
    state.mesh_ipv6 = new_ipv6;
    state.peers.clear();
    store::save(&state)?;

    ui::step_ok(&sp, "Secret rotated");
    ui::info_line("New secret", &new_secret.to_string());
    ui::info_line("New IPv6", &new_ipv6.to_string());
    println!();
    println!("All peers must rejoin with the new secret.");
    println!("Restart this node with 'syfrah fabric start'.");
    Ok(())
}
