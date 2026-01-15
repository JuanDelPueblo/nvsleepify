# nvsleepify

`nvsleepify` is a lightweight Rust CLI tool designed for Linux users who want to manually control the power state of their Nvidia dGPU. It effectively "sleeps" (powers off) and "wakes" (powers on) the GPU on demand, which is particularly useful for laptops or hybrid graphics setups where battery life or noise control is a priority.

## Features

-   **Check Status**: Quickly see if your Nvidia GPU is Active (D0), Suspended, or Powered Off (D3cold).
-   **Safe Disable (`on`)**:
    -   Detects and warns about processes using the GPU.
    -   Stops Nvidia systemd services (`nvidia-persistenced`, `nvidia-powerd`).
    -   Unloads kernel modules (`nvidia`, `nvidia_uvm`, `nvidia_modeset`, `nvidia_drm`).
    -   Unbinds the PCI driver.
    -   Cuts power to the PCI slot.
-   **Safe Enable (`off`)**:
    -   Powers on the PCI slot.
    -   Rescans the PCI bus to rediscover the device.
    -   Reloads kernel modules.
    -   Restarts Nvidia services.

## Prerequisites

-   **Linux OS** (Tested with systemd distributions).
-   **Nvidia Proprietary Drivers** (The tool expects standard Nvidia module names).
-   **Root Privileges**: Access to `/sys/bus/pci` and service management requires `sudo`.
-   **Dependencies**:
    -   `lsof` (for detecting processes using the GPU).
    -   `systemd` (for managing services).

## Installation

### From Source

You can build and install `nvsleepify` using the provided Makefile.

```bash
# Clone the repository
git clone https://github.com/yourusername/nvsleepify.git
cd nvsleepify

# Build and Install (installs to /usr/local/bin by default)
sudo make install
```

To uninstall:

```bash
sudo make uninstall
```

Alternatively, checking out the code and running with cargo:

```bash
cargo build --release
sudo ./target/release/nvsleepify status
```

## Usage

**Note:** Most commands require root privileges to interact with hardware and system services.

### Check Status
```bash
sudo nvsleepify status
```
Displays the current PCI address, power state (D0/D3cold), and lists any device nodes or blocking processes.

### Sleep GPU (Turn Off)
```bash
sudo nvsleepify on
```
This command performs the shutdown sequence. If processes are found using the GPU, it will prompt you interactively to kill them or abort.

### Wake GPU (Turn On)
```bash
sudo nvsleepify off
```
This command reverses the shutdown sequence, powering on the slot, rescanning the bus, and reloading drivers.

## Troubleshooting

-   **Slot Power Control**: This tool writes to `/sys/bus/pci/slots/*/power`. If your hardware/BIOS does not expose PCI slots control via ACPI, this feature might not work.
-   **Modules Busy**: If `nvsleepify on` fails to unload modules, ensure no background processes (like Xorg or Wayland compositors bound to the Nvidia card) are running.
-   **Permission Denied**: Ensure you are running with `sudo`.

