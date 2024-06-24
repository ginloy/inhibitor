use clap::{Parser, Subcommand};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Option<CliSubcommand>,
}

#[derive(Subcommand)]
enum CliSubcommand {
    /// Query for inhibitor state
    Query,
    /// Activate idle inhibition
    On,
    /// Deactivate idle inhibition
    Off,
}

fn main() {
    let args = Cli::parse();
}
