use crate::store;
use anyhow::Result;

pub async fn run() -> Result<()> {
    let state = store::load().map_err(|_| {
        anyhow::anyhow!(
            "no mesh configured. Run 'syfrah fabric init' or 'syfrah fabric join' first."
        )
    })?;
    println!("{}", state.mesh_secret);
    Ok(())
}
