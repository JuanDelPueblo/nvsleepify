use anyhow::{anyhow, Context, Result};
use colored::*;
use std::process::Command;

pub fn is_user_logged_in() -> bool {
    // Check if any user with UID >= 1000 has a session using loginctl
    if let Ok(output) = Command::new("loginctl")
        .arg("list-users")
        .arg("--no-legend")
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            // Output format: " UID USER"
            if let Some(uid_str) = parts.get(0) {
                if let Ok(uid) = uid_str.parse::<u32>() {
                    // Filter out system users (typically UID < 1000)
                    if uid >= 1000 && uid < 65534 {
                        return true;
                    }
                }
            }
        }
    }

    // Fallback: check /run/user for any active user runtime directories with UID >= 1000
    if let Ok(entries) = std::fs::read_dir("/run/user") {
        for entry in entries.flatten() {
            if let Ok(file_name) = entry.file_name().into_string() {
                if let Ok(uid) = file_name.parse::<u32>() {
                    if uid >= 1000 && uid < 65534 {
                        return true;
                    }
                }
            }
        }
    }

    false
}

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
            if name.starts_with("nvidia-po") || name.starts_with("nvidia-pe") {
                continue;
            }
            procs.push((name.to_string(), parts[1].to_string()));
        }
    }
    Ok(procs)
}

pub fn kill_processes(procs: &[(String, String)]) -> Result<()> {
    for (_, pid) in procs {
        let _ = Command::new("kill").arg("-15").arg(pid).status();
    }
    Ok(())
}

fn run_systemctl(action: &str, service: &str) {
    match Command::new("systemctl").arg(action).arg(service).status() {
        Ok(status) => {
            if !status.success() {
                eprintln!(
                    "{} Failed to {} {}: {}",
                    "WARN:".yellow(),
                    action,
                    service,
                    status
                );
            }
        }
        Err(e) => {
            eprintln!(
                "{} Failed to execute systemctl {} {}: {}",
                "WARN:".yellow(),
                action,
                service,
                e
            );
        }
    }
}

pub fn stop_services() -> Result<()> {
    println!("{}", "Stopping systemd services...".blue());
    let services = ["nvidia-persistenced", "nvidia-powerd"];
    for svc in services {
        run_systemctl("stop", svc);
    }

    let services_to_disable = [
        "nvidia-suspend.service",
        "nvidia-hibernate.service",
        "nvidia-resume.service",
        "nvidia-persistenced.service",
        "nvidia-powerd.service",
    ];
    for svc in services_to_disable {
        run_systemctl("disable", svc);
    }

    // Mask nvidia-fallback.service to prevent it from interfering
    run_systemctl("stop", "nvidia-fallback.service");
    run_systemctl("mask", "nvidia-fallback.service");

    Ok(())
}

pub fn start_services() -> Result<()> {
    println!("{}", "Starting systemd services...".blue());

    // Unmask nvidia-fallback.service
    run_systemctl("unmask", "nvidia-fallback.service");

    let services = ["nvidia-persistenced", "nvidia-powerd"];
    for svc in services {
        run_systemctl("start", svc);
    }

    let services_to_enable = [
        "nvidia-suspend.service",
        "nvidia-hibernate.service",
        "nvidia-resume.service",
        "nvidia-persistenced.service",
        "nvidia-powerd.service",
    ];
    for svc in services_to_enable {
        run_systemctl("enable", svc);
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

pub fn get_charging_status() -> bool {
    let candidates = [
        "/sys/class/power_supply/ACAD/online",
        "/sys/class/power_supply/AC/online",
        "/sys/class/power_supply/ADP1/online",
    ];
    for path in candidates {
        if let Ok(content) = std::fs::read_to_string(path) {
            return content.trim() == "1";
        }
    }
    // Fallback: If we genuinely can't tell, assume charging to be safe (never sleep unwantedly)
    true
}
