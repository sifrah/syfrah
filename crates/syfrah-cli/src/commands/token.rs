use anyhow::Result;
use syfrah_net::store;

pub async fn run() -> Result<()> {
    let state = store::load().map_err(|_| {
        anyhow::anyhow!("no mesh configured. Run 'syfrah init' or 'syfrah join' first.")
    })?;
    println!("{}", state.mesh_secret);
    Ok(())
}
