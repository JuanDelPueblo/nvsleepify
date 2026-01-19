use crate::pci::PciDevice;
use crate::system;
use anyhow::Result;

use std::fmt::Write;
use tokio::task::spawn_blocking;
use zbus::{dbus_interface, ConnectionBuilder};

const STATE_FILE: &str = "/var/lib/nvsleepify/state";
const AUTO_FILE: &str = "/var/lib/nvsleepify/auto";

struct NvSleepifyManager;

#[dbus_interface(name = "org.nvsleepify.Manager")]
impl NvSleepifyManager {
    async fn status(&self) -> String {
        spawn_blocking(move || status_logic())
            .await
            .unwrap_or_else(|e| format!("Internal error: {}", e))
    }

    /// Read-only info for UIs.
    /// Returns: (sleep_enabled, auto_enabled, power_state, blocking_processes)
    async fn info(&self) -> (bool, bool, String, Vec<(String, String)>) {
        spawn_blocking(move || info_logic())
            .await
            .unwrap_or_else(|e| (false, false, format!("Internal error: {}", e), vec![]))
    }

    /// Sleep the GPU.
    /// Returns: (success, message, blocking_processes)
    async fn sleep(&self, kill_procs: bool) -> (bool, String, Vec<(String, String)>) {
        // Disabling auto mode when manual command is issued is a good UX pattern,
        // but the user didn't explicitly ask for it. However, if I manually sleep,
        // and auto mode thinks I should be awake (plugged in), it will just wake me up again in 5s.
        // User asked: "wait 5 seconds after any charging changes before turning on/off the gpu"
        // It implies auto mode reacts to charging changes.
        // If I manually sleep while plugged in and auto is on, auto loop detects "Charging" + "Sleep Enabled" -> Wake.
        // So manual commands are overridden by auto mode. This is acceptable for "Auto".
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

    /// Set Auto Mode
    async fn set_auto(&self, enable: bool) -> String {
        spawn_blocking(move || set_auto_logic(enable))
            .await
            .unwrap_or_else(|e| format!("Internal error: {}", e))
    }
}

async fn monitor_loop() {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));

    // Auto mode state
    let mut last_charging = system::get_charging_status();
    let mut stable_since = tokio::time::Instant::now();

    loop {
        interval.tick().await;

        // --- Auto/Charging Logic ---
        let auto_enabled = spawn_blocking(|| load_auto_state()).await.unwrap_or(false);

        if auto_enabled {
            let current_charging = spawn_blocking(|| system::get_charging_status())
                .await
                .unwrap_or(true);

            if current_charging != last_charging {
                println!(
                    "Monitor: Power state changed to {}. Debouncing...",
                    if current_charging {
                        "Charging"
                    } else {
                        "Unplugged"
                    }
                );
                last_charging = current_charging;
                stable_since = tokio::time::Instant::now();
            } else if stable_since.elapsed().as_secs() >= 5 {
                // Stable for 5 seconds
                let sleep_enabled = spawn_blocking(|| load_state()).await.unwrap_or(false);

                if current_charging {
                    // Plugged in -> Should be Awake (sleep_enabled == false)
                    if sleep_enabled {
                        println!("Monitor: Auto-mode enforcing WAKE (Plugged in)");
                        let _ = spawn_blocking(|| wake_logic()).await;
                    }
                } else {
                    // Unplugged -> Should be Asleep (sleep_enabled == true)
                    if !sleep_enabled {
                        println!("Monitor: Auto-mode enforcing SLEEP (Unplugged)");
                        // Soft sleep
                        let _ = spawn_blocking(|| sleep_logic(false)).await;
                    }
                }
            }
        }

        // --- Existing Sleep Enforcement Logic ---

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

fn info_logic() -> (bool, bool, String, Vec<(String, String)>) {
    let sleep_enabled = load_state();
    let auto_enabled = load_auto_state();

    match PciDevice::find_nvidia_gpu() {
        Ok(gpu) => {
            let nodes = gpu.get_device_nodes();
            let power_state = gpu.get_power_state();
            let procs = system::get_processes_using_nvidia(&nodes).unwrap_or_default();
            (sleep_enabled, auto_enabled, power_state, procs)
        }
        Err(_) => (sleep_enabled, auto_enabled, "NotFound".to_string(), vec![]),
    }
}

fn status_logic() -> String {
    let mut output = String::new();
    let auto_enabled = load_auto_state();
    if auto_enabled {
        writeln!(output, "Auto Mode:   Enabled").unwrap();
    } else {
        writeln!(output, "Auto Mode:   Disabled").unwrap();
    }

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
                println!("Sleep blocked by processes (soft-sleep): {:?}", procs);
                return (false, "Blocking processes found".to_string(), procs);
            }
            // If we are about to force kill, we should save state as 'on'
            // so the monitor loop enforces it if we crash/fail halfway,
            // but we can also just save it down below.
            if let Err(e) = system::kill_processes(&procs) {
                return (false, format!("Failed to kill processes: {}", e), vec![]);
            }
            // Give time for processes to die
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        Err(e) => return (false, format!("Failed checking processes: {}", e), vec![]),
        _ => {}
    }

    if let Err(e) = save_state(true) {
        return (false, format!("Failed to save state: {}", e), vec![]);
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

fn save_auto_state(enabled: bool) -> Result<()> {
    let path = std::path::Path::new(AUTO_FILE);
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let content = if enabled { "true" } else { "false" };
    std::fs::write(path, content)?;
    Ok(())
}

fn load_auto_state() -> bool {
    let path = std::path::Path::new(AUTO_FILE);
    if let Ok(content) = std::fs::read_to_string(path) {
        return content.trim() == "true";
    }
    false
}

fn set_auto_logic(enable: bool) -> String {
    if let Err(e) = save_auto_state(enable) {
        return format!("Failed to save auto state: {}", e);
    }
    if enable {
        "Auto mode enabled. Monitor will manage GPU power.".to_string()
    } else {
        "Auto mode disabled.".to_string()
    }
}
