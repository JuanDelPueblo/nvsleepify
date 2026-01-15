use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PciDevice {
    pub address: String,
    pub path: PathBuf,
}

impl PciDevice {
    pub fn new(address: &str) -> Self {
        Self {
            address: address.to_string(),
            path: PathBuf::from(format!("/sys/bus/pci/devices/{}", address)),
        }
    }

    pub fn unbind_driver(&self) -> Result<()> {
        let driver_path = self.path.join("driver/unbind");
        if !driver_path.exists() {
            // Already unbound
            return Ok(());
        }
        // echo address > driver/unbind
        fs::write(driver_path, &self.address)?;
        Ok(())
    }

    pub fn get_slot_path(&self) -> Option<PathBuf> {
        // Try to find physical slot in /sys/bus/pci/slots
        // This is heuristic; sometimes there's a 'slot' file in the device dir
        // containing the number.

        // 1. Check if 'slot' file exists in device dir
        let slot_file = self.path.join("slot");
        if let Ok(slot_num) = fs::read_to_string(&slot_file) {
            let slot_num = slot_num.trim();
            let pci_slots = Path::new("/sys/bus/pci/slots");
            if pci_slots.exists() {
                for entry in fs::read_dir(pci_slots).ok()? {
                    let entry = entry.ok()?;
                    let path = entry.path();
                    // Some systems use the address as the slot name, some use numbers
                    // If we found a number in 'slot' file, look for directory with that number
                    if path.file_name()?.to_string_lossy() == slot_num {
                        return Some(path);
                    }
                }

                // If that failed, sometimes the slot directory name IS the number in the slot file
                let candidate = pci_slots.join(slot_num);
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }

        // Fallback: This is tricky. The user mentioned "/sys/bus/pci/slots/0/power".
        // Users might need to configure this manually if autodetection fails.
        // Or we can try to walk up the bridge.
        None
    }

    pub fn set_slot_power(&self, on: bool) -> Result<()> {
        let slot_dir = self.get_slot_path().ok_or_else(|| {
            anyhow!(
                "Could not find PCI slot for device {}. (Is acpiphp loaded?)",
                self.address
            )
        })?;
        let power_file = slot_dir.join("power");
        let val = if on { "1" } else { "0" };

        // If the power file doesn't exist, we can't control slot power.
        if !power_file.exists() {
            return Err(anyhow!(
                "Slot power control file not found at {:?}",
                power_file
            ));
        }

        fs::write(&power_file, val).context("Failed to write to slot power file")?;
        Ok(())
    }

    pub fn rescan() -> Result<()> {
        fs::write("/sys/bus/pci/rescan", "1").context("Failed to rescan PCI bus")?;
        Ok(())
    }

    pub fn find_nvidia_gpu() -> Result<Self> {
        let pci_root = Path::new("/sys/bus/pci/devices");
        for entry in fs::read_dir(pci_root)? {
            let entry = entry?;
            let path = entry.path();
            let vendor_path = path.join("vendor");
            let device_path = path.join("device"); // Device ID, not needed strictly if we trust vendor

            if vendor_path.exists() {
                let vendor = fs::read_to_string(vendor_path)?;
                if vendor.trim() == "0x10de" {
                    // Check class to ensure it's a VGA/display controller (0x03)
                    // or 3D controller (0x0302).
                    // Subclasses are in 'class' file (0x030000, etc)
                    let class_path = path.join("class");
                    let class = fs::read_to_string(class_path)?;
                    if class.starts_with("0x03") {
                        let address = path.file_name().unwrap().to_string_lossy().to_string();
                        return Ok(PciDevice::new(&address));
                    }
                }
            }
        }
        Err(anyhow!("No Nvidia GPU found on PCI bus"))
    }

    pub fn get_power_state(&self) -> String {
        let path = self.path.join("power_state");
        fs::read_to_string(path)
            .unwrap_or_else(|_| "Unknown".to_string())
            .trim()
            .to_string()
    }

    pub fn get_device_nodes(&self) -> Vec<String> {
        let mut nodes = Vec::new();
        // Check drm dir: /sys/bus/pci/devices/.../drm/cardX/
        let drm_path = self.path.join("drm");
        if drm_path.exists() {
            if let Ok(entries) = fs::read_dir(drm_path) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with("card") || name.starts_with("render") {
                        nodes.push(format!("/dev/dri/{}", name));
                    }
                }
            }
        }
        nodes
    }
}
