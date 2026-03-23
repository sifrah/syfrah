pub mod drop;
pub mod get;
pub mod list;

use clap::Subcommand;

#[derive(Subcommand)]
pub enum StateCommand {
    /// List tables in a layer's state database
    List(list::Args),
    /// Get values from a table in a layer's state database
    Get(get::Args),
    /// Drop (delete) an entire layer's state database
    Drop(drop::Args),
}

pub async fn run(cmd: StateCommand) -> anyhow::Result<()> {
    match cmd {
        StateCommand::List(args) => list::run(args).await,
        StateCommand::Get(args) => get::run(args).await,
        StateCommand::Drop(args) => drop::run(args).await,
    }
}
