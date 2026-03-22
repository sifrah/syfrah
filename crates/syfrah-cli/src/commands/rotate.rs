use anyhow::Result;
use syfrah_core::secret::MeshSecret;
use syfrah_net::store;

pub async fn run() -> Result<()> {
    let mut state = store::load().map_err(|_| {
        anyhow::anyhow!("no mesh configured.")
    })?;

    if store::daemon_running().is_some() {
        anyhow::bail!("daemon is running. Stop it first with 'syfrah stop'.");
    }

    let new_secret = MeshSecret::generate();
    let new_prefix = syfrah_net::daemon::derive_prefix_from_secret(&new_secret);
    let new_ipv6 = syfrah_core::addressing::derive_node_address(
        &new_prefix,
        wireguard_control::Key::from_base64(&state.wg_public_key)
            .map_err(|_| anyhow::anyhow!("bad WG key"))?.as_bytes(),
    );

    state.mesh_secret = new_secret.to_string();
    state.mesh_prefix = new_prefix;
    state.mesh_ipv6 = new_ipv6;
    state.peers.clear();
    store::save(&state)?;

    println!("Secret rotated.");
    println!("  New secret: {new_secret}");
    println!("  New IPv6:   {new_ipv6}");
    println!();
    println!("All peers must rejoin with the new secret.");
    println!("Restart this node with 'syfrah start'.");
    Ok(())
}
