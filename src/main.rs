mod pci;
mod system;

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::*;
use pci::PciDevice;
use std::process;

#[derive(Parser)]
#[command(name = "nvsleepify")]
#[command(about = "Manage Nvidia dGPU power state (sleep/wake) on Linux", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Get GPU status
    Status,
    /// Disable (Sleep) the Nvidia GPU
    On,
    /// Enable (Wake) the Nvidia GPU
    Off,
}

fn main() -> Result<()> {
    if unsafe { libc::geteuid() } != 0 {
         println!("{}", "Warning: This program usually requires root privileges to control PCI power and services.".yellow());
         println!("If commands fail, try running with sudo.");
    }

    let cli = Cli::parse();

    match cli.command {
        Commands::Status => status_command()?,
        Commands::On => on_command()?,
        Commands::Off => off_command()?,
    }

    Ok(())
}

fn status_command() -> Result<()> {
    match PciDevice::find_nvidia_gpu() {
        Ok(gpu) => {
            println!("Nvidia GPU Found:");
            println!("  PCI Address: {}", gpu.address.green());
            println!("  PCI Path:    {:?}", gpu.path);
            let nodes = gpu.get_device_nodes();
            if !nodes.is_empty() {
                println!("  Device Nodes: {}", nodes.join(", ").blue());
            } else {
                println!("  Device Nodes: {}", "None (Driver unbound or card off)".yellow());
            }

            let state = gpu.get_power_state();
            let state_colored = if state == "D0" { state.green() } else { state.blue() };
            println!("  Power State: {}", state_colored);
            
            let procs = system::get_processes_using_nvidia().unwrap_or_default();
            if !procs.is_empty() {
                println!("  Status: {}", "Active (In Use)".red());
                println!("  Blocking Processes: {}", procs.len());
            } else if state == "D3cold" {
                println!("  Status: {}", "Off / D3cold".blue());
            } else if state.contains("D3") {
                 println!("  Status: {}", "Suspended".yellow());
            } else {
                 println!("  Status: {}", "Idle / D0".green());
            }
        },
        Err(_) => {
            println!("{}", "No Nvidia GPU running on PCI bus (or currently hidden/powered off).".red());
            println!("If you previously ran 'nvsleepify on', run 'nvsleepify off' to enable it.");
        }
    }
    Ok(())
}

fn on_command() -> Result<()> {
    println!("{}", "=== Enabling Sleep Mode (Turning GPU OFF) ===".bold());
    
    // 1. Find GPU
    let gpu = match PciDevice::find_nvidia_gpu() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("{}", e);
            println!("{}", "Continuing assuming GPU might be already off or not found...".yellow());
            return Ok(());
        }
    };
    println!("Found GPU at {}", gpu.address.cyan());
    let nodes = gpu.get_device_nodes();
    if !nodes.is_empty() {
        println!("  Device Nodes: {:?}", nodes);
    }

    // 2. Check/Kill Processes
    system::check_and_kill_processes()?;

    // 3. Stop Services
    system::stop_services()?;

    // 4. Unload Modules
    system::unload_modules()?;

    // 5. Unbind Driver
    println!("Unbinding driver...");
    gpu.unbind_driver()?;

    // 6. Power Off Slot
    println!("Powering off PCI slot...");
    if let Err(e) = gpu.set_slot_power(false) {
        eprintln!("{}", format!("Failed to set slot power: {}.\nMake sure you have write access to /sys/bus/pci/slots/*/power (run as root).", e).red());
    } else {
        println!("{}", "GPU Powered OFF.".green());
    }

    Ok(())
}

fn off_command() -> Result<()> {
    println!("{}", "=== Disabling Sleep Mode (Turning GPU ON) ===".bold());
    
    // 1. Power On Slot & Find Slots
    // Try to find any disabled slots and turn them on.
    use std::fs;
    let slots_dir = std::path::Path::new("/sys/bus/pci/slots");
    let mut turned_on = false;
    if slots_dir.exists() {
        if let Ok(entries) = fs::read_dir(slots_dir) {
            for entry in entries.flatten() {
                let power_path = entry.path().join("power");
                if power_path.exists() {
                    let content = fs::read_to_string(&power_path).unwrap_or_default();
                    // Some systems use "0" for off, "1" for on
                    if content.trim() == "0" {
                        println!("Found slot {:?} powered OFF. Powering ON...", entry.path());
                        if let Err(e) = fs::write(&power_path, "1") {
                             eprintln!("Failed to power on slot: {}", e);
                        } else {
                             turned_on = true;
                        }
                    }
                }
            }
        }
    }
    
    // 2. Rescan
    println!("Rescanning PCI bus...");
    PciDevice::rescan()?;
    
    // Wait a bit
    std::thread::sleep(std::time::Duration::from_secs(1));

    // 3. Load Modules (This usually handles driver binding too)
    // Reverse order: Power -> (Bind? No, Load Modules creates driver) -> Modules -> Services.
    system::load_modules()?;

    // 4. Check if GPU appeared
    let gpu = match PciDevice::find_nvidia_gpu() {
        Ok(g) => {
            println!("GPU found at {}.", g.address.cyan());
            g 
        },
        Err(_) => {
             println!("{}", "Warning: GPU not found on bus yet. It might take more time or reboot.".yellow());
             // We continue to start services just in case
             PciDevice::new("0000:00:00.0") // dummy
        }
    };
    
    // 5. Start Services
    system::start_services()?;

    println!("{}", "GPU Powered ON and Services Started.".green());
    Ok(())
}

