# nvsleepify

`nvsleepify` is a lightweight tool written in Rust designed for Linux users who want their Nvidia dGPU to stay off. It effectively "sleeps" (powers off) and "wakes" (powers on) the GPU on demand, which is useful to prevent programs randomly waking up the dGPU wasting battery life.

## Features

-   **Check Status**: Quickly see if your Nvidia GPU is Active (D0), Suspended, or Powered Off (D3cold).
-   **Integrated Mode (`integrated`)**:
    -   Forces the Nvidia dGPU off to save power.
    -   Detects and warns about processes using the GPU.
    -   Stops Nvidia systemd services (`nvidia-persistenced`, `nvidia-powerd`).
    -   Unloads kernel modules (`nvidia`, `nvidia_uvm`, `nvidia_modeset`, `nvidia_drm`).
    -   Unbinds the PCI driver and cuts power to the PCI slot.
-   **Standard Mode (`standard`)**:
    -   Ensures the GPU is available for use.
    -   Powers on the PCI slot, rescans the bus, reloads kernel modules, and restarts services.
-   **Optimized Mode (`optimized`)**:
    -   Automatically switches between Standard (when plugged in) and Integrated (on battery) modes.
-   **Systemd Service**: Restores the last saved nvsleepify mode on boot or resume.
-   **Tray Applet**: View GPU status and switch modes from within your DE's system tray.

## Prerequisites

-   **Linux OS with Systemd**
-   **Nvidia dGPU**
-   **Dependencies**:
    -   `lsof` (for detecting processes using the GPU).
    -   `systemd` (for managing services).

## Installation

### From Source

You can build and install `nvsleepify` using the provided Makefile.

```bash
# Clone the repository
git clone https://github.com/JuanDelPueblo/nvsleepify.git
cd nvsleepify

# Build and Install
make
sudo make install
```

To uninstall:

```bash
sudo make uninstall
```

## Usage

To use this tool you must enable the `nvsleepifyd` service as follows:

```bash
sudo systemctl enable --now nvsleepifyd.service
```

### Tray Applet

This project comes with a tray applet called `nvsleepify-tray` which lets you control your GPU from within your DE's system tray. It comes with icons for different states (active, suspended, off), a right click menu to switch between modes (Standard, Integrated, Optimized), and notifications for when the GPU changes state.

### CLI commands

#### Check Status
```bash
nvsleepify status
```
Displays the current PCI address, power state (D0/D3cold), and lists any device nodes or blocking processes.

#### Set Mode
Change the operation mode of the daemon.

**Integrated (Force Off):**
```bash
nvsleepify set integrated
```
Forces the shutdown sequence. If processes are using the GPU, it may fail or require confirmation (if run interactively or via tray).

**Standard (Always On):**
```bash
nvsleepify set standard
```
Reverses the shutdown sequence, ensuring the GPU is powered and drivers are loaded.

**Optimized (Auto):**
```bash
nvsleepify set optimized
```
Enables automatic power management based on charging status (Wake on AC, Sleep on Battery).

## Notes

-    I wrote this program for personal use on an Asus Zephyrus G14 2024 running Fedora. I cannot guarantee this program will function correctly on your system, but if you encounter any issues let me know and I can try helping fix any issues when I have time.
-    If using KDE Plasma, add these environment variables to `/etc/environment` to ensure Kwin doesn't hold the dGPU hostage if you use external displays

```bash
KWIN_DRM_DEVICES=/dev/dri/card0:/dev/dri/card1 # where the first card is the iGPU and the second card is the dGPU
__EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json
__GLX_VENDOR_LIBRARY_NAME=mesa
VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/radeon_icd.x86_64.json # different for Intel iGPUs
```
-    At least on my laptop, with the above environment variables KDE Powerdevil will hang and crash if you don't disable DDC in `~/.config/powerdevilrc` by adding

```
[Backlight]
EnableDDC=false
```
-    On Fedora, you must deactivate the `nvidia-settings-user.desktop` autostart entry found in `/etc/xdg/autostart` by copying it to ~/.config/autostart/ and setting the `Hidden` field to `true`. This prevents the GPU from waking up on initial boot up.

## References used

* https://github.com/Bensikrac/VFIO-Nvidia-dynamic-unbind
* https://gitlab.com/asus-linux/supreme-chainsaw/-/wikis/Manual-Steps-Guide
