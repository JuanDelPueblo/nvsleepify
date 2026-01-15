use anyhow::{anyhow, Context, Result};
use colored::*;
use dialoguer::Select;
use std::process::Command;

pub fn get_processes_using_nvidia(extra_paths: &[String]) -> Result<Vec<(String, String)>> {
    // Basic nvidia paths that are always relevant
    // We will use sh to run lsof with glob pattern for /dev/nvidia*
    // And append specific DRI paths provided by caller

    let mut paths_to_check = vec!["/dev/nvidia[0-9]*".to_string()];
    paths_to_check.extend_from_slice(extra_paths);

    let path_args = paths_to_check.join(" ");

    let output = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "lsof -w {} | grep -v PID | awk '{{print $1, $2}}' | sort -u",
            path_args
        ))
        .output()
        .context("Failed to run lsof")?;

    let stdout = String::from_utf8(output.stdout)?;
    let mut procs = Vec::new();
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let name = parts[0];
            // Ignore nvidia-powerd (shows as nvidia-po) as it's a service we stop gracefully
            if name.starts_with("nvidia-po") {
                continue;
            }
            procs.push((name.to_string(), parts[1].to_string()));
        }
    }
    Ok(procs)
}

pub fn check_and_kill_processes(extra_paths: &[String]) -> Result<()> {
    loop {
        let procs = get_processes_using_nvidia(extra_paths)?;
        if procs.is_empty() {
            break;
        }

        println!("{}", "Processes using Nvidia GPU found:".yellow());
        for (name, pid) in &procs {
            println!("  {} (PID: {})", name, pid);
        }

        let options = vec!["Refresh", "Kill found processes", "Abort"];
        let selection = Select::new()
            .with_prompt("What do you want to do?")
            .items(&options)
            .default(0)
            .interact()?;

        match selection {
            0 => continue, // Refresh
            1 => {
                for (_, pid) in procs {
                    let _ = Command::new("kill").arg("-9").arg(&pid).status();
                }
                println!("Processes killed.");
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            _ => return Err(anyhow!("Operation aborted by user")),
        }
    }
    Ok(())
}

pub fn stop_services() -> Result<()> {
    println!("{}", "Stopping systemd services...".blue());
    let services = ["nvidia-persistenced", "nvidia-powerd"];
    for svc in services {
        let _ = Command::new("systemctl").arg("stop").arg(svc).status();
    }
    Ok(())
}

pub fn start_services() -> Result<()> {
    println!("{}", "Starting systemd services...".blue());
    let services = ["nvidia-persistenced", "nvidia-powerd"];
    for svc in services {
        let _ = Command::new("systemctl").arg("start").arg(svc).status();
    }
    Ok(())
}

pub fn unload_modules() -> Result<()> {
    println!("{}", "Unloading kernel modules...".blue());
    // Order matters: nvidia_uvm, nvidia_modeset, nvidia_drm, nvidia
    // Dependencies: drm depends on nvidia, modeset depends on nvidia...
    // To be safe, try `modprobe -r nvidia_drm nvidia_modeset nvidia_uvm nvidia`

    let status = Command::new("modprobe")
        .arg("-r")
        .arg("nvidia_drm")
        .arg("nvidia_modeset")
        .arg("nvidia_uvm")
        .arg("nvidia")
        .status()?;

    if !status.success() {
        return Err(anyhow!("Failed to unload nvidia modules. Check if stuck or used by other processes (e.g. Xorg, Wayland)."));
    }
    Ok(())
}

pub fn load_modules() -> Result<()> {
    println!("{}", "Loading kernel modules...".blue());
    let status = Command::new("modprobe")
        .arg("nvidia")
        .arg("nvidia_uvm")
        .arg("nvidia_modeset")
        .arg("nvidia_drm")
        .status()?;

    if !status.success() {
        return Err(anyhow!("Failed to load nvidia modules."));
    }
    Ok(())
}
