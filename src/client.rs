use crate::protocol::{Command, Response};
use anyhow::{anyhow, Result};
use colored::*;
use dialoguer::Select;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

const SOCKET_PATH: &str = "/run/nvsleepify.sock";

pub async fn run(mut command: Command) -> Result<()> {
    loop {
        let mut stream = UnixStream::connect(SOCKET_PATH).await.map_err(|e| {
            anyhow!(
                "Failed to connect to daemon at {}: {}. Is nvsleepifyd.service running?",
                SOCKET_PATH,
                e
            )
        })?;

        let bytes = serde_json::to_vec(&command)?;
        stream.write_all(&bytes).await?;

        let mut buf = vec![0; 4096];
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            return Err(anyhow!("Daemon closed connection"));
        }
        let resp: Response = serde_json::from_slice(&buf[..n])?;

        match resp {
            Response::Ok => {
                println!("{}", "Success.".green());
                break;
            }
            Response::Error(e) => {
                println!("{}", format!("Error: {}", e).red());
                break;
            }
            Response::StatusOutput(s) => {
                print!("{}", s);
                break;
            }
            Response::ProcessesRunning(procs) => {
                println!("{}", "Processes using Nvidia GPU found:".yellow());
                for (name, pid) in &procs {
                    println!("  {} (PID: {})", name, pid);
                }

                let options = vec!["Cancel", "Kill processes and sleep"];
                let selection = Select::new()
                    .with_prompt("Blocking processes found. Action?")
                    .items(&options)
                    .default(0)
                    .interact()?;

                if selection == 1 {
                    command = Command::Sleep { kill_procs: true };
                    continue; // Reconnect and send new command
                } else {
                    break;
                }
            }
        }
    }
    Ok(())
}
