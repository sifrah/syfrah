use clap::Args as ClapArgs;

use crate::{db_path, LayerDb};

#[derive(ClapArgs)]
pub struct Args {
    /// Layer name (e.g., fabric, compute, overlay)
    pub layer: String,

    /// Skip confirmation prompt
    #[arg(long)]
    pub force: bool,
}

pub async fn run(args: Args) -> anyhow::Result<()> {
    let path = db_path(&args.layer);
    if !path.exists() {
        println!("No state database for layer '{}'.", args.layer);
        return Ok(());
    }

    if !args.force {
        println!(
            "This will permanently delete all state for layer '{}'.",
            args.layer
        );
        println!("File: {}", path.display());
        print!("Continue? [y/N] ");

        use std::io::Write;
        std::io::stdout().flush().ok();
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let trimmed = input.trim().to_lowercase();
        if trimmed != "y" && trimmed != "yes" {
            println!("Aborted.");
            return Ok(());
        }
    }

    LayerDb::destroy(&args.layer)?;
    println!("State for layer '{}' deleted.", args.layer);
    Ok(())
}
