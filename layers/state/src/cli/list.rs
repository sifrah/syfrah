use clap::Args as ClapArgs;

use crate::{db_path, LayerDb};

#[derive(ClapArgs)]
pub struct Args {
    /// Layer name (e.g., fabric, compute, overlay)
    pub layer: String,
}

pub async fn run(args: Args) -> anyhow::Result<()> {
    let path = db_path(&args.layer);
    if !path.exists() {
        anyhow::bail!(
            "no state database for layer '{}' (expected {})",
            args.layer,
            path.display()
        );
    }

    let db = LayerDb::open(&args.layer)?;

    // redb doesn't expose a "list tables" API directly.
    // We try well-known table names and report which exist.
    let known_tables = [
        "config",
        "peers",
        "metrics",
        "vms",
        "vpcs",
        "subnets",
        "ip_allocations",
        "volumes",
        "images",
        "raft_log",
        "raft_state",
        "orgs",
        "projects",
        "environments",
        "users",
        "roles",
        "apikeys",
    ];

    println!("Layer: {}", args.layer);
    println!("File:  {}", path.display());
    println!();

    let mut found = 0;
    for table_name in &known_tables {
        let count = db.count(table_name)?;
        if count > 0 {
            println!("  {:<20} {} entries", table_name, count);
            found += 1;
        }
    }

    if found == 0 {
        println!("  (no tables with data)");
    }

    Ok(())
}
