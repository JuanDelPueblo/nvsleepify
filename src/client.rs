use crate::protocol::Command;
use anyhow::{anyhow, Result};
use colored::*;
use zbus::{dbus_proxy, Connection};

#[dbus_proxy(
    interface = "org.nvsleepify.Manager",
    default_service = "org.nvsleepify.Service",
    default_path = "/org/nvsleepify/Manager"
)]
trait NvSleepifyManager {
    fn status(&self) -> zbus::Result<String>;
    fn info(&self) -> zbus::Result<(String, String, Vec<(String, String)>)>;
    fn set_mode(&self, mode_str: String) -> zbus::Result<(bool, String, Vec<(String, String)>)>;
}

pub async fn run(command: Command) -> Result<()> {
    let connection = Connection::system()
        .await
        .map_err(|e| anyhow!("Failed to connect to system bus: {}. Is dbus running?", e))?;

    let proxy = NvSleepifyManagerProxy::new(&connection).await.map_err(|e| {
         anyhow!("Failed to connect to nvsleepify daemon at org.nvsleepify.Service: {}. Is nvsleepifyd.service running?", e)
    })?;

    match command {
        Command::Status => {
            let status = proxy.status().await?;
            print!("{}", status);
        }
        Command::Set(mode) => {
            let (success, msg, procs) = proxy.set_mode(mode.to_string()).await?;

            if success {
                println!("Set mode to {}: {}", mode, "Success.".green());
            } else {
                 if !procs.is_empty() {
                    println!("{}", "Processes using Nvidia GPU found:".yellow());
                    for (name, pid) in &procs {
                        println!("  {} (PID: {})", name, pid);
                    }
                 }
                 println!("{}", format!("Error: {}", msg).red());
            }
        }
    }
    Ok(())
}
