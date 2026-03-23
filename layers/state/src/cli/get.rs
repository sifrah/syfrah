use clap::Args as ClapArgs;

use crate::{db_path, LayerDb};

#[derive(ClapArgs)]
pub struct Args {
    /// Layer name (e.g., fabric, compute, overlay)
    pub layer: String,

    /// Table name (e.g., peers, config, metrics)
    pub table: String,

    /// Optional key to get a specific entry. If omitted, lists all entries.
    pub key: Option<String>,
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

    // Special case: metrics table stores u64, not JSON
    if args.table == "metrics" {
        return dump_metrics(&db, args.key.as_deref()).await;
    }

    match args.key {
        Some(key) => {
            // Get a specific entry
            let value: Option<serde_json::Value> = db.get(&args.table, &key)?;
            match value {
                Some(v) => {
                    println!("{}", serde_json::to_string_pretty(&v)?);
                }
                None => {
                    anyhow::bail!("key '{}' not found in table '{}'", key, args.table);
                }
            }
        }
        None => {
            // List all entries in the table
            let entries: Vec<(String, serde_json::Value)> = db.list(&args.table)?;
            if entries.is_empty() {
                println!("(empty table)");
            } else {
                for (key, value) in &entries {
                    println!("── {} ──", key);
                    println!("{}", serde_json::to_string_pretty(value)?);
                    println!();
                }
                println!("{} entries", entries.len());
            }
        }
    }

    Ok(())
}

async fn dump_metrics(db: &LayerDb, key: Option<&str>) -> anyhow::Result<()> {
    let known_metrics = [
        "peers_discovered",
        "wg_reconciliations",
        "peers_marked_unreachable",
        "daemon_started_at",
    ];

    match key {
        Some(k) => {
            let val = db.get_metric(k)?;
            println!("{}: {}", k, val);
        }
        None => {
            let mut found = false;
            for m in &known_metrics {
                let val = db.get_metric(m)?;
                if val > 0 {
                    println!("  {:<30} {}", m, val);
                    found = true;
                }
            }
            if !found {
                println!("(no metrics set)");
            }
        }
    }

    Ok(())
}
