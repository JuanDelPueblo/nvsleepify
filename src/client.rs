use crate::protocol::{Command, Mode};
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
    fn set_restore_delay(&self, seconds: u32) -> zbus::Result<String>;
}

fn confirm_kill_processes(procs: &[(String, String)]) -> bool {
    if procs.is_empty() {
        return true;
    }

    let mut text = String::new();
    text.push_str("The following processes are using the Nvidia GPU and may need to be killed to sleep it:\n\n");
    for (name, pid) in procs {
        text.push_str(&format!("- {} (PID {})\n", name, pid));
    }

    let result = rfd::MessageDialog::new()
        .set_title("nvsleepify")
        .set_description(&text)
        .set_buttons(rfd::MessageButtons::YesNo)
        .show();

    matches!(result, rfd::MessageDialogResult::Yes)
}

fn confirm_kill_processes_cli(procs: &[(String, String)]) -> bool {
    if procs.is_empty() {
        return true;
    }

    println!("{}", "The following processes are using the Nvidia GPU and may need to be killed to sleep it:".yellow());
    for (name, pid) in procs {
        println!("- {} (PID {})", name, pid);
    }
    println!();

    dialoguer::Confirm::new()
        .with_prompt("Do you want to proceed?")
        .default(false)
        .interact()
        .unwrap_or(false)
}

pub async fn run(command: Command, use_gui: bool) -> Result<()> {
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
            if mode == Mode::Integrated {
                let (_, _, processes) = proxy.info().await?;
                if !processes.is_empty() {
                    let confirmed = if use_gui {
                        confirm_kill_processes(&processes)
                    } else {
                        confirm_kill_processes_cli(&processes)
                    };

                    if !confirmed {
                        println!("Aborted by user.");
                        return Ok(());
                    }
                }
            }

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
        Command::Delay(seconds) => {
            let msg = proxy.set_restore_delay(seconds).await?;
            println!("{}", msg);
        }
    }
    Ok(())
}
