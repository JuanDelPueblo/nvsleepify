use anyhow::Result;
use clap::{Parser, Subcommand};
use clap_complete::Shell;
use nvsleepify::{
    client,
    protocol::{Command, Mode},
};

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
        /// Show GUI confirmation dialog if processes need to be killed
        #[arg(long)]
        gui: bool,
    },
    /// Set delay before restoring GPU state on boot
    Delay {
        /// Delay in seconds
        seconds: u32,
    },
    /// Generate shell completions
    Completion {
        #[arg(value_enum)]
        shell: Shell,
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

    let (cmd, gui) = match command_enum {
        Commands::Status => (Command::Status, false),
        Commands::Set { mode, gui } => (Command::Set(mode), gui),
        Commands::Delay { seconds } => (Command::Delay(seconds), false),
        Commands::Completion { shell } => {
            use clap::CommandFactory;
            clap_complete::generate(
                shell,
                &mut Cli::command(),
                "nvsleepify",
                &mut std::io::stdout(),
            );
            return Ok(());
        }
    };

    client::run(cmd, gui).await
}
