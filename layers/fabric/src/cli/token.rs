use crate::{no_mesh_error, store};
use anyhow::Result;

pub async fn run() -> Result<()> {
    let state = store::load().map_err(|_| no_mesh_error())?;
    eprintln!("Warning: this is a sensitive credential. Do not share publicly.");
    println!("{}", state.mesh_secret);
    Ok(())
}
