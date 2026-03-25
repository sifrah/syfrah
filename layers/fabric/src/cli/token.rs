use crate::{no_mesh_error, store};
use anyhow::Result;

pub async fn run() -> Result<()> {
    let state = store::load().map_err(|_| no_mesh_error())?;
    println!("{}", state.mesh_secret);
    Ok(())
}
