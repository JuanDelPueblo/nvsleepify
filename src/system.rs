use anyhow::{anyhow, Context, Result};
use colored::*;
use dialoguer::{Confirm, Select};
use regex::Regex;
use std::process::Command;

pub fn get_processes_using_nvidia() -> Result<Vec<(String, String)>> {
    // Run lsof, parse output
    let output = Command::new("lsof")
        .arg("-t") // terse, just PIDs
        .arg("/dev/nvidia0")
        .arg("/dev/nvidiactl")
        .arg("/dev/nvidia-modeset")
        // Add more common paths or use shell globbing (Command does not glob)
        // Since we can't easily glob with Command, we might need to invoke via sh
        .output();

    // A better approach is to use `fuser -v /dev/nvidia*` or iterate /proc
    // But user suggested `lsof`.
    // Let's use `lsof` with a shell generic list if possible, or common specific devices.
    // Or iterate over /dev/nvidia*

    // We will use sh to run lsof with glob pattern
    let output = Command::new("sh")
        .arg("-c")
        .arg("lsof -w /dev/nvidia* /dev/dri/card* /dev/dri/renderD* | grep -v PID | awk '{print $1, $2}' | sort -u")
        .output()
        .context("Failed to run lsof")?;

    let stdout = String::from_utf8(output.stdout)?;
    let mut procs = Vec::new();
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            procs.push((parts[0].to_string(), parts[1].to_string()));
        }
    }
    Ok(procs)
}

pub fn check_and_kill_processes() -> Result<()> {
    loop {
        let procs = get_processes_using_nvidia()?;
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
