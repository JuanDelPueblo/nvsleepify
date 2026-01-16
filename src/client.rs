use crate::protocol::Command;
use anyhow::{anyhow, Result};
use colored::*;
use dialoguer::Select;
use zbus::{dbus_proxy, Connection};

#[dbus_proxy(
    interface = "org.nvsleepify.Manager",
    default_service = "org.nvsleepify.Service",
    default_path = "/org/nvsleepify/Manager"
)]
trait NvSleepifyManager {
    fn status(&self) -> zbus::Result<String>;
    fn info(&self) -> zbus::Result<(bool, String, Vec<(String, String)>)>;
    fn sleep(&self, kill_procs: bool) -> zbus::Result<(bool, String, Vec<(String, String)>)>;
    fn wake(&self) -> zbus::Result<(bool, String)>;
}

pub async fn run(mut command: Command) -> Result<()> {
    let connection = Connection::system()
        .await
        .map_err(|e| anyhow!("Failed to connect to system bus: {}. Is dbus running?", e))?;

    let proxy = NvSleepifyManagerProxy::new(&connection).await.map_err(|e| {
         anyhow!("Failed to connect to nvsleepify daemon at org.nvsleepify.Service: {}. Is nvsleepifyd.service running?", e)
    })?;

    loop {
        match command {
            Command::Status => {
                let status = proxy.status().await?;
                print!("{}", status);
                break;
            }
            Command::Wake => {
                let (success, msg) = proxy.wake().await?;
                if success {
                    println!("{}", "Success.".green());
                } else {
                    println!("{}", format!("Error: {}", msg).red());
                }
                break;
            }
            Command::Sleep { kill_procs } => {
                let (success, msg, procs) = proxy.sleep(kill_procs).await?;

                if success {
                    println!("{}", "Success.".green());
                    break;
                }

                if !procs.is_empty() && msg.contains("processes") {
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
                        continue; // Re-run loop with kill_procs = true
                    } else {
                        break;
                    }
                } else {
                    println!("{}", format!("Error: {}", msg).red());
                    break;
                }
            }
        }
    }
    Ok(())
}
