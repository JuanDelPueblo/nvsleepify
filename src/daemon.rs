use crate::pci::PciDevice;
use crate::protocol::Mode;
use crate::system;
use anyhow::Result;

use std::fmt::Write;
use std::str::FromStr;
use tokio::task::spawn_blocking;
use zbus::{dbus_interface, ConnectionBuilder};

const MODE_FILE: &str = "/var/lib/nvsleepify/mode";
const DELAY_FILE: &str = "/var/lib/nvsleepify/restore_delay";

struct NvSleepifyManager;

#[dbus_interface(name = "org.nvsleepify.Manager")]
impl NvSleepifyManager {
    async fn status(&self) -> String {
        spawn_blocking(move || status_logic())
            .await
            .unwrap_or_else(|e| format!("Internal error: {}", e))
    }

    /// Read-only info for UIs.
    /// Returns: (mode_str, power_state, blocking_processes)
    async fn info(&self) -> (String, String, Vec<(String, String)>) {
        spawn_blocking(move || info_logic())
            .await
            .unwrap_or_else(|e| {
                (
                    "Unknown".to_string(),
                    format!("Internal error: {}", e),
                    vec![],
                )
            })
    }

    /// Set Mode.
    async fn set_mode(&self, mode_str: String) -> (bool, String, Vec<(String, String)>) {
        spawn_blocking(move || set_mode_logic(&mode_str))
            .await
            .unwrap_or_else(|e| (false, format!("Internal error: {}", e), vec![]))
    }

    /// Set restore delay in seconds.
    async fn set_restore_delay(&self, seconds: u32) -> String {
        spawn_blocking(move || save_delay(seconds))
            .await
            .unwrap_or_else(|e| Err(anyhow::anyhow!("Internal error: {}", e)))
            .map(|_| format!("Restore delay set to {} seconds", seconds))
            .unwrap_or_else(|e| format!("Failed to set delay: {}", e))
    }
}

async fn monitor_loop() {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));

    let mut last_charging = system::get_charging_status();
    let mut stable_since = tokio::time::Instant::now();

    loop {
        interval.tick().await;

        let mode = spawn_blocking(|| load_mode())
            .await
            .unwrap_or(Ok(Mode::Standard))
            .unwrap_or(Mode::Standard);

        match mode {
            Mode::Optimized => {
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
                } else if stable_since.elapsed().as_secs() >= 2 {
                    if current_charging {
                        let already_active = spawn_blocking(|| {
                            if PciDevice::find_nvidia_gpu().is_ok() {
                                // Check if nvidia module is loaded
                                if let Ok(content) = std::fs::read_to_string("/proc/modules") {
                                    return content.contains("nvidia ");
                                }
                            }
                            false
                        })
                        .await
                        .unwrap_or(false);

                        if !already_active {
                            let _ = spawn_blocking(|| wake_logic()).await;
                        }
                    } else {
                        let _ = spawn_blocking(|| sleep_logic(false)).await;
                    }
                }
            }
            Mode::Integrated => {
                let should_sleep = spawn_blocking(|| match PciDevice::find_nvidia_gpu() {
                    Ok(gpu) => {
                        let state = gpu.get_power_state();
                        state == "D0" || state == "Unknown"
                    }
                    Err(_) => false,
                })
                .await
                .unwrap_or(false);

                if should_sleep {
                    println!("Monitor: GPU detected in high power state while Integrated mode is active. Attempting to disable...");
                    let _ = spawn_blocking(|| sleep_logic(true)).await;
                }
            }
            Mode::Standard => {}
            Mode::Vfio => {}
        }
    }
}

pub async fn run() -> Result<()> {
    println!("Starting NvSleepify D-Bus daemon...");

    // Wait for user login
    println!("Waiting for user login...");
    loop {
        let logged_in = spawn_blocking(|| system::is_user_logged_in())
            .await
            .unwrap_or(false);
        if logged_in {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }
    println!("User logged in detected.");

    // Restore state on startup
    println!("Restoring previous state...");
    let delay = spawn_blocking(|| load_delay())
        .await
        .unwrap_or(Ok(0))
        .unwrap_or(0);
    if delay > 0 {
        println!("Waiting {} seconds before restoring state...", delay);
        tokio::time::sleep(tokio::time::Duration::from_secs(delay as u64)).await;
    }

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

fn save_mode(mode: Mode) -> Result<()> {
    let path = std::path::Path::new(MODE_FILE);
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let content = match mode {
        Mode::Standard => "standard",
        Mode::Integrated => "integrated",
        Mode::Optimized => "optimized",
        Mode::Vfio => "vfio",
    };
    std::fs::write(path, content)?;
    Ok(())
}

fn load_mode() -> Result<Mode> {
    let path = std::path::Path::new(MODE_FILE);
    if !path.exists() {
        return Ok(Mode::Standard);
    }
    let content = std::fs::read_to_string(path)?;
    Mode::from_str(content.trim()).map_err(|e| anyhow::anyhow!(e))
}

fn save_delay(seconds: u32) -> Result<()> {
    let path = std::path::Path::new(DELAY_FILE);
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(path, seconds.to_string())?;
    Ok(())
}

fn load_delay() -> Result<u32> {
    let path = std::path::Path::new(DELAY_FILE);
    if !path.exists() {
        return Ok(0);
    }
    let content = std::fs::read_to_string(path)?;
    content
        .trim()
        .parse::<u32>()
        .map_err(|e| anyhow::anyhow!(e))
}

fn info_logic() -> (String, String, Vec<(String, String)>) {
    let mode = load_mode().unwrap_or(Mode::Standard);
    let mode_str = mode.to_string();

    match PciDevice::find_nvidia_gpu() {
        Ok(gpu) => {
            let nodes = gpu.get_device_nodes();
            let power_state = gpu.get_power_state();
            let procs = system::get_processes_using_nvidia(&nodes).unwrap_or_default();
            (mode_str, power_state, procs)
        }
        Err(_) => (mode_str, "NotFound".to_string(), vec![]),
    }
}

fn status_logic() -> String {
    let mut output = String::new();
    let mode = load_mode().unwrap_or(Mode::Standard);
    writeln!(output, "Current Mode: {}", mode).unwrap();

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
        }
    }
    output
}

fn set_mode_logic(mode_str: &str) -> (bool, String, Vec<(String, String)>) {
    let mode = match Mode::from_str(mode_str) {
        Ok(m) => m,
        Err(e) => return (false, format!("Invalid mode: {}", e), vec![]),
    };

    if let Err(e) = save_mode(mode) {
        return (false, format!("Failed to save mode: {}", e), vec![]);
    }

    // Attempt to unload VFIO modules when switching to any mode other than VFIO
    if mode != Mode::Vfio {
        let _ = system::unload_vfio_modules();
    }

    match mode {
        Mode::Standard => {
            let (success, msg) = wake_logic();
            (success, msg, vec![])
        }
        Mode::Integrated => sleep_logic(true),
        Mode::Vfio => setup_vfio_logic(),
        Mode::Optimized => {
            if system::get_charging_status() {
                let (success, msg) = wake_logic();
                (success, msg, vec![])
            } else {
                sleep_logic(false)
            }
        }
    }
}

fn setup_vfio_logic() -> (bool, String, Vec<(String, String)>) {
    // 1. Ensure GPU is powered on (to read IDs)
    // Like wake_logic but without loading nvidia modules
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

    // 2. Find GPU and Audio to get IDs
    let gpu = match PciDevice::find_nvidia_gpu() {
        Ok(g) => g,
        Err(_) => return (false, "GPU not found even after power on".to_string(), vec![]),
    };
    
    // 3. Kill processes using GPU (if any, e.g. from Standard mode)
    let nodes = gpu.get_device_nodes();
    if let Ok(procs) = system::get_processes_using_nvidia(&nodes) {
        if !procs.is_empty() {
             if let Err(e) = system::kill_processes(&procs) {
                return (false, format!("Failed to kill processes: {}", e), vec![]);
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

    // 4. Stop Nvidia Services & Unload Nvidia Modules
    if let Err(e) = system::stop_services() {
        return (false, format!("Failed to stop services: {}", e), vec![]);
    }
    if let Err(e) = system::unload_modules() {
        return (false, format!("Failed to unload modules: {}", e), vec![]);
    }
    // Unbind driver if still attached
    let _ = gpu.unbind_driver();
    
    // Get IDs
    let vendor = gpu.get_vendor_id().unwrap_or_default();
    let device = gpu.get_device_id().unwrap_or_default();
    let mut ids_str = format!("{}:{}", vendor, device);
    
    if let Some(audio) = gpu.find_related_audio_device() {
        let _ = audio.unbind_driver();
        let a_vendor = audio.get_vendor_id().unwrap_or_default();
        let a_device = audio.get_device_id().unwrap_or_default();
        if !a_vendor.is_empty() && !a_device.is_empty() {
            ids_str.push_str(&format!(",{}:{}", a_vendor, a_device));
        }
    }

    // 5. Load VFIO modules with IDs
    if let Err(e) = system::load_vfio_modules(Some(&ids_str)) {
        return (false, format!("Failed to load VFIO modules: {}", e), vec![]);
    }
    
    // 6. Bind to VFIO explicitly if not picked up?
    // Usually modprobe with ids does it if device is unbound.
    // We can try to bind manually just in case.
    let _ = gpu.bind_driver("vfio-pci");
    if let Some(audio) = gpu.find_related_audio_device() {
        let _ = audio.bind_driver("vfio-pci");
    }

    (true, format!("Switched to VFIO mode with IDs: {}", ids_str), vec![])
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
            if let Err(e) = system::kill_processes(&procs) {
                return (false, format!("Failed to kill processes: {}", e), vec![]);
            }
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
    let mode = load_mode().unwrap_or(Mode::Standard);
    
    // Ensure cleanup of VFIO if not in VFIO mode
    if mode != Mode::Vfio {
        let _ = system::unload_vfio_modules();
    }

    match mode {
        Mode::Standard => {
            wake_logic();
        }
        Mode::Integrated => {
            sleep_logic(true);
        }
        Mode::Vfio => {
            setup_vfio_logic();
        }
        Mode::Optimized => {
            if system::get_charging_status() {
                wake_logic();
            } else {
                sleep_logic(false);
            }
        }
    }
    Ok(())
}
