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

        // Fallback: Iterate over all slots and match based on address
        // This handles cases where 'slot' file is missing in device directory
        let pci_slots = Path::new("/sys/bus/pci/slots");
        if pci_slots.exists() {
            if let Ok(entries) = fs::read_dir(pci_slots) {
                for entry in entries.flatten() {
                    let address_file = entry.path().join("address");
                    if let Ok(addr_content) = fs::read_to_string(address_file) {
                        let addr_content = addr_content.trim();
                        // self.address is normally "0000:01:00.0"
                        // addr_content in slot is usually "0000:01:00" (bus address)
                        // Verify if device address starts with the slot address
                        if !addr_content.is_empty()
                            && (self.address == addr_content
                                || self.address.starts_with(addr_content))
                        {
                            return Some(entry.path());
                        }
                    }
                }
            }
        }

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
            // let device_path = path.join("device"); // Device ID, not needed strictly if we trust vendor

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

    pub fn find_related_audio_device(&self) -> Option<Self> {
        let pci_root = Path::new("/sys/bus/pci/devices");
        if let Ok(entries) = fs::read_dir(pci_root) {
            for entry in entries.flatten() {
                let path = entry.path();
                let addr_str_os = path.file_name()?;
                let addr_str = addr_str_os.to_string_lossy();

                // address format: DDDD:BB:DD.F (len >= 12 usually)
                if addr_str.len() > 2 && self.address.len() > 2 {
                    // Check if it shares the same domain:bus:slot prefix
                    // e.g. "0000:01:00.0" -> "0000:01:00"
                    let gpu_slot_prefix = &self.address[..self.address.len() - 2];
                    let current_prefix = &addr_str[..addr_str.len() - 2];

                    if gpu_slot_prefix == current_prefix && addr_str != self.address {
                        // Check if it is an audio device (0x0403)
                        if let Ok(content) = fs::read_to_string(path.join("class")) {
                            if content.starts_with("0x0403") {
                                return Some(PciDevice::new(&addr_str));
                            }
                        }
                    }
                }
            }
        }
        None
    }

    pub fn get_vendor_id(&self) -> Result<String> {
        let path = self.path.join("vendor");
        let content = fs::read_to_string(path)?;
        Ok(content.trim().trim_start_matches("0x").to_string())
    }

    pub fn get_device_id(&self) -> Result<String> {
        let path = self.path.join("device");
        let content = fs::read_to_string(path)?;
        Ok(content.trim().trim_start_matches("0x").to_string())
    }

    pub fn bind_driver(&self, driver: &str) -> Result<()> {
        let driver_path = Path::new("/sys/bus/pci/drivers").join(driver);
        if !driver_path.exists() {
            return Err(anyhow!("Driver {} not found", driver));
        }
        let bind_path = driver_path.join("bind");
        fs::write(bind_path, &self.address)?;
        Ok(())
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
