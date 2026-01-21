use anyhow::Result;
use clap::{Parser, Subcommand};
use nvsleepify::{client, protocol::{Command, Mode}};

#[derive(Parser)]
#[command(name = "nvsleepify")]
#[command(about = "Manage Nvidia dGPU power state (sleep/wake) on Linux", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Get GPU status
    Status,
    /// Set power management mode
    Set {
        #[arg(value_enum)]
        mode: Mode,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Default to displaying help if no subcommand is provided
    let command_enum = match cli.command {
        Some(c) => c,
        None => {
            use clap::CommandFactory;
            Cli::command().print_help()?;
            return Ok(());
        }
    };

    let cmd = match command_enum {
        Commands::Status => Command::Status,
        Commands::Set { mode } => Command::Set(mode),
    };

    client::run(cmd).await
}
