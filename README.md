# nvsleepify

`nvsleepify` is a lightweight tool written in Rust designed for Linux users who want their Nvidia dGPU to stay off. It effectively "sleeps" (powers off) and "wakes" (powers on) the GPU on demand, which is useful to prevent programs randomly waking up the dGPU wasting battery life.

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
-   **Systemd Service**: Restores the last saved nvsleepify mode on boot or resume.

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

### Check Status
```bash
nvsleepify status
```
Displays the current PCI address, power state (D0/D3cold), and lists any device nodes or blocking processes.

### Sleep GPU (Turn Off)
```bash
nvsleepify on
```
This command performs the shutdown sequence. If processes are found using the GPU, it will prompt you interactively to kill them or abort.

### Wake GPU (Turn On)
```bash
nvsleepify off
```
This command reverses the shutdown sequence, powering on the slot, rescanning the bus, and reloading drivers.

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
-    On Fedora, you must mask `nvidia-fallback.service` using systemctl and deactivate the `nvidia-settings-user.desktop` autostart entry found in `/etc/xdg/autostart` by copying it to ~/.config/autostart/ and setting the `Hidden` field to `true`. This prevents the GPU from waking up on sleep and on initial boot up.

## References used

* https://github.com/Bensikrac/VFIO-Nvidia-dynamic-unbind
* https://gitlab.com/asus-linux/supreme-chainsaw/-/wikis/Manual-Steps-Guide
