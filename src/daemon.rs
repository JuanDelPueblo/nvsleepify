use crate::pci::PciDevice;
use crate::protocol::{Command, Response};
use crate::system;
use anyhow::Result;
use futures_util::StreamExt;
use std::fmt::Write;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tokio::task::spawn_blocking;
use zbus::dbus_proxy;

const SOCKET_PATH: &str = "/run/nvsleepify.sock";
const STATE_FILE: &str = "/var/lib/nvsleepify/state";

#[dbus_proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
trait LoginManager {
    #[dbus_proxy(signal)]
    fn prepare_for_sleep(&self, start: bool) -> zbus::Result<()>;
}

pub async fn run() -> Result<()> {
    if std::fs::metadata(SOCKET_PATH).is_ok() {
        let _ = std::fs::remove_file(SOCKET_PATH);
    }

    let listener = UnixListener::bind(SOCKET_PATH)?;

    // Set permissions to 666 so anyone can connect
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(SOCKET_PATH, std::fs::Permissions::from_mode(0o666))?;

    println!("Daemon listening on {}", SOCKET_PATH);

    // Spawn sleep monitor
    tokio::spawn(async {
        if let Err(e) = monitor_sleep_signal().await {
            eprintln!("Sleep monitor error: {}", e);
        }
    });

    loop {
        match listener.accept().await {
            Ok((mut socket, _addr)) => {
                tokio::spawn(async move {
                    let mut buf = vec![0; 4096];
                    let n = match socket.read(&mut buf).await {
                        Ok(n) if n == 0 => return,
                        Ok(n) => n,
                        Err(_) => return,
                    };

                    let req: Command = match serde_json::from_slice(&buf[..n]) {
                        Ok(c) => c,
                        Err(e) => {
                            let _ = socket
                                .write_all(
                                    &serde_json::to_vec(&Response::Error(e.to_string())).unwrap(),
                                )
                                .await;
                            return;
                        }
                    };

                    let resp = handle_command(req);
                    let resp_bytes = serde_json::to_vec(&resp).unwrap();
                    let _ = socket.write_all(&resp_bytes).await;
                });
            }
            Err(e) => eprintln!("Accept error: {}", e),
        }
    }
}

async fn monitor_sleep_signal() -> Result<()> {
    let connection = zbus::Connection::system().await?;
    let manager = LoginManagerProxy::new(&connection).await?;
    let mut stream = manager.receive_prepare_for_sleep().await?;

    while let Some(signal) = stream.next().await {
        match signal.args() {
            Ok(args) => {
                if !args.start {
                    println!("System resumed from sleep. Waiting 5s before restore...");
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

                    println!("Restoring state...");
                    let _ = spawn_blocking(|| match restore() {
                        Response::Ok => println!("Restore successful"),
                        Response::Error(e) => eprintln!("Restore failed: {}", e),
                        _ => {}
                    })
                    .await;
                }
            }
            Err(e) => eprintln!("Error parsing signal args: {}", e),
        }
    }
    Ok(())
}

fn handle_command(cmd: Command) -> Response {
    match cmd {
        Command::Status => status(),
        Command::Sleep { kill_procs } => sleep(kill_procs),
        Command::Wake => wake(),
    }
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

fn status() -> Response {
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
    Response::StatusOutput(output)
}

fn sleep(kill_procs: bool) -> Response {
    if let Err(e) = save_state(true) {
        return Response::Error(format!("Failed to save state: {}", e));
    }

    let gpu = match PciDevice::find_nvidia_gpu() {
        Ok(g) => g,
        Err(_) => return Response::Ok, // Already off
    };

    let nodes = gpu.get_device_nodes();
    match system::get_processes_using_nvidia(&nodes) {
        Ok(procs) if !procs.is_empty() => {
            if !kill_procs {
                return Response::ProcessesRunning(procs);
            }
            if let Err(e) = system::kill_processes(&procs) {
                return Response::Error(format!("Failed to kill processes: {}", e));
            }
            // Give time for processes to die
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        Err(e) => return Response::Error(format!("Failed to checking processes: {}", e)),
        _ => {}
    }

    if let Err(e) = system::stop_services() {
        return Response::Error(format!("Failed to stop services: {}", e));
    }
    if let Err(e) = system::unload_modules() {
        return Response::Error(format!("Failed to unload modules: {}", e));
    }
    if let Err(e) = gpu.unbind_driver() {
        return Response::Error(format!("Failed to unbind driver: {}", e));
    }
    if let Err(e) = gpu.set_slot_power(false) {
        return Response::Error(format!("Failed to power off slot: {}", e));
    }

    Response::Ok
}

fn wake() -> Response {
    if let Err(e) = save_state(false) {
        return Response::Error(format!("Failed to save state: {}", e));
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
        return Response::Error(format!("Failed to load modules: {}", e));
    }

    // Check if GPU appeared (optional check) but we proceed anyway

    if let Err(e) = system::start_services() {
        return Response::Error(format!("Failed to start services: {}", e));
    }

    Response::Ok
}

fn restore() -> Response {
    let path = std::path::Path::new(STATE_FILE);
    if !path.exists() {
        return Response::Ok;
    }
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c.trim().to_string(),
        Err(e) => return Response::Error(e.to_string()),
    };

    if content == "on" {
        sleep(true) // Force sleep on restore
    } else {
        wake()
    }
}
