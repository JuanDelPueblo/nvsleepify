use crate::pci::PciDevice;
use crate::system;
use anyhow::Result;

use std::fmt::Write;
use tokio::task::spawn_blocking;
use zbus::{dbus_interface, ConnectionBuilder};

const STATE_FILE: &str = "/var/lib/nvsleepify/state";

struct NvSleepifyManager;

#[dbus_interface(name = "org.nvsleepify.Manager")]
impl NvSleepifyManager {
    async fn status(&self) -> String {
        spawn_blocking(move || status_logic())
            .await
            .unwrap_or_else(|e| format!("Internal error: {}", e))
    }

    /// Read-only info for UIs.
    /// Returns: (sleep_enabled, power_state, blocking_processes)
    async fn info(&self) -> (bool, String, Vec<(String, String)>) {
        spawn_blocking(move || info_logic())
            .await
            .unwrap_or_else(|e| (false, format!("Internal error: {}", e), vec![]))
    }

    /// Sleep the GPU.
    /// Returns: (success, message, blocking_processes)
    async fn sleep(&self, kill_procs: bool) -> (bool, String, Vec<(String, String)>) {
        spawn_blocking(move || sleep_logic(kill_procs))
            .await
            .unwrap_or_else(|e| (false, format!("Internal error: {}", e), vec![]))
    }

    /// Wake the GPU.
    /// Returns: (success, message)
    async fn wake(&self) -> (bool, String) {
        spawn_blocking(move || wake_logic())
            .await
            .unwrap_or_else(|e| (false, format!("Internal error: {}", e)))
    }
}

async fn monitor_loop() {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
    loop {
        interval.tick().await;

        let should_sleep = spawn_blocking(|| {
            if !load_state() {
                return false;
            }
            // Check if GPU is present and awake
            match PciDevice::find_nvidia_gpu() {
                Ok(gpu) => {
                    let state = gpu.get_power_state();
                    // If we see D0 or Unknown, treat it as "Awake"
                    state == "D0" || state == "Unknown"
                }
                Err(_) => {
                    // Not found means it's likely powered off or safe
                    false
                }
            }
        })
        .await
        .unwrap_or(false);

        if should_sleep {
            println!("Monitor: GPU detected in high power state while sleep is enabled. Attempting to disable...");
            let res = spawn_blocking(|| sleep_logic(true)).await;
            match res {
                Ok((true, _, _)) => println!("Monitor: Successfully enforced sleep."),
                Ok((false, msg, _)) => eprintln!("Monitor: Failed to enforce sleep: {}", msg),
                Err(e) => eprintln!("Monitor: Internal error executing sleep logic: {}", e),
            }
        }
    }
}

pub async fn run() -> Result<()> {
    println!("Starting NvSleepify D-Bus daemon...");

    // Restore state on startup
    println!("Restoring previous state...");
    let _ = spawn_blocking(|| match restore_logic() {
        Ok(_) => println!("State restore successful"),
        Err(e) => eprintln!("State restore failed: {}", e),
    })
    .await;

    // Start background monitoring
    tokio::spawn(monitor_loop());

    // Setup D-Bus connection
    let _conn = ConnectionBuilder::system()?
        .name("org.nvsleepify.Service")?
        .serve_at("/org/nvsleepify/Manager", NvSleepifyManager)?
        .build()
        .await?;

    println!("Daemon listening on system bus: org.nvsleepify.Service");

    // Keep running indefinitely (the connection will handle incoming messages)
    std::future::pending::<()>().await;
    Ok(())
}

fn save_state(sleep_enabled: bool) -> Result<()> {
    let path = std::path::Path::new(STATE_FILE);
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let content = if sleep_enabled { "on" } else { "off" };
    std::fs::write(path, content)?;
    Ok(())
}

fn load_state() -> bool {
    let path = std::path::Path::new(STATE_FILE);
    if let Ok(content) = std::fs::read_to_string(path) {
        return content.trim() == "on";
    }
    false
}

fn info_logic() -> (bool, String, Vec<(String, String)>) {
    let sleep_enabled = load_state();

    match PciDevice::find_nvidia_gpu() {
        Ok(gpu) => {
            let nodes = gpu.get_device_nodes();
            let power_state = gpu.get_power_state();
            let procs = system::get_processes_using_nvidia(&nodes).unwrap_or_default();
            (sleep_enabled, power_state, procs)
        }
        Err(_) => (sleep_enabled, "NotFound".to_string(), vec![]),
    }
}

fn status_logic() -> String {
    let mut output = String::new();
    match PciDevice::find_nvidia_gpu() {
        Ok(gpu) => {
            writeln!(output, "Nvidia GPU Found:").unwrap();
            writeln!(output, "  PCI Address: {}", gpu.address).unwrap();
            writeln!(output, "  PCI Path:    {:?}", gpu.path).unwrap();
            let nodes = gpu.get_device_nodes();
            if !nodes.is_empty() {
                writeln!(output, "  Device Nodes: {}", nodes.join(", ")).unwrap();
            } else {
                writeln!(output, "  Device Nodes: None (Driver unbound or card off)").unwrap();
            }

            let state = gpu.get_power_state();
            writeln!(output, "  Power State: {}", state).unwrap();

            let procs = system::get_processes_using_nvidia(&nodes).unwrap_or_default();
            if !procs.is_empty() {
                writeln!(output, "  Status: Active (In Use)").unwrap();
                writeln!(output, "  Blocking Processes: {}", procs.len()).unwrap();
            } else if state == "D3cold" {
                writeln!(output, "  Status: Off / D3cold").unwrap();
            } else if state.contains("D3") {
                writeln!(output, "  Status: Suspended").unwrap();
            } else {
                writeln!(output, "  Status: Idle / D0").unwrap();
            }
        }
        Err(_) => {
            writeln!(
                output,
                "No Nvidia GPU running on PCI bus (or currently hidden/powered off)."
            )
            .unwrap();
            writeln!(
                output,
                "If you previously ran 'nvsleepify on', run 'nvsleepify off' to enable it."
            )
            .unwrap();
        }
    }
    output
}

fn sleep_logic(kill_procs: bool) -> (bool, String, Vec<(String, String)>) {
    if let Err(e) = save_state(true) {
        return (false, format!("Failed to save state: {}", e), vec![]);
    }

    let gpu = match PciDevice::find_nvidia_gpu() {
        Ok(g) => g,
        Err(_) => {
            return (
                true,
                "Nvidia GPU not found (already off?)".to_string(),
                vec![],
            )
        }
    };

    let nodes = gpu.get_device_nodes();
    match system::get_processes_using_nvidia(&nodes) {
        Ok(procs) if !procs.is_empty() => {
            if !kill_procs {
                return (false, "Blocking processes found".to_string(), procs);
            }
            if let Err(e) = system::kill_processes(&procs) {
                return (false, format!("Failed to kill processes: {}", e), vec![]);
            }
            // Give time for processes to die
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        Err(e) => return (false, format!("Failed checking processes: {}", e), vec![]),
        _ => {}
    }

    if let Err(e) = system::stop_services() {
        return (false, format!("Failed to stop services: {}", e), vec![]);
    }
    if let Err(e) = system::unload_modules() {
        return (false, format!("Failed to unload modules: {}", e), vec![]);
    }
    if let Err(e) = gpu.unbind_driver() {
        return (false, format!("Failed to unbind driver: {}", e), vec![]);
    }
    if let Err(e) = gpu.set_slot_power(false) {
        return (false, format!("Failed to power off slot: {}", e), vec![]);
    }

    (true, "Success".to_string(), vec![])
}

fn wake_logic() -> (bool, String) {
    if let Err(e) = save_state(false) {
        return (false, format!("Failed to save state: {}", e));
    }

    use std::fs;
    let slots_dir = std::path::Path::new("/sys/bus/pci/slots");
    if slots_dir.exists() {
        if let Ok(entries) = fs::read_dir(slots_dir) {
            for entry in entries.flatten() {
                let power_path = entry.path().join("power");
                if power_path.exists() {
                    let content = fs::read_to_string(&power_path).unwrap_or_default();
                    if content.trim() == "0" {
                        let _ = fs::write(&power_path, "1");
                    }
                }
            }
        }
    }

    let _ = PciDevice::rescan();
    std::thread::sleep(std::time::Duration::from_secs(1));

    if let Err(e) = system::load_modules() {
        return (false, format!("Failed to load modules: {}", e));
    }

    if let Err(e) = system::start_services() {
        return (false, format!("Failed to start services: {}", e));
    }

    (true, "Success".to_string())
}

fn restore_logic() -> Result<()> {
    let path = std::path::Path::new(STATE_FILE);
    if !path.exists() {
        return Ok::<(), anyhow::Error>(());
    }
    let content = std::fs::read_to_string(path)?.trim().to_string();

    if content == "on" {
        sleep_logic(true); // Force sleep
    } else {
        wake_logic();
    }
    Ok(())
}
