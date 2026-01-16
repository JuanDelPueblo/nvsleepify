use anyhow::Result;
use clap::{Parser, Subcommand};
use nvsleepify::{client, protocol::Command};

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
    /// Disable (Sleep) the Nvidia GPU
    On {
        /// Force kill blocking processes without confirmation
        #[arg(short, long)]
        force: bool,
    },
    /// Enable (Wake) the Nvidia GPU
    Off,
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
        Commands::On { force } => Command::Sleep { kill_procs: force },
        Commands::Off => Command::Wake,
    };

    client::run(cmd).await
}
