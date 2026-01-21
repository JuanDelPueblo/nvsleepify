use anyhow::{anyhow, Result};
use ksni::TrayMethods;
use notify_rust::Notification;
use nvsleepify::protocol::Mode;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use zbus::{dbus_proxy, Connection};

#[dbus_proxy(
    interface = "org.nvsleepify.Manager",
    default_service = "org.nvsleepify.Service",
    default_path = "/org/nvsleepify/Manager"
)]
trait NvSleepifyManager {
    fn status(&self) -> zbus::Result<String>;
    fn info(&self) -> zbus::Result<(String, String, Vec<(String, String)>)>;
    fn set_mode(&self, mode_str: String) -> zbus::Result<(bool, String, Vec<(String, String)>)>;
}

#[derive(Debug, Clone, Copy)]
enum TrayCommand {
    SetMode(Mode),
    ToggleNotifications,
    Quit,
}

#[derive(Debug, Default, Clone)]
struct UiState {
    mode: Mode,
    power_state: String,
    processes: Vec<(String, String)>,
    last_error: Option<String>,
}

#[derive(Debug)]
struct NvSleepifyTray {
    state: UiState,
    notifications_enabled: Arc<AtomicBool>,
    tx: mpsc::UnboundedSender<TrayCommand>,
}

impl NvSleepifyTray {
    fn icon_name_for_state(state: &UiState) -> String {
        if !state.processes.is_empty() {
            return "nvsleepify-gpu-active".into();
        }
        if state.power_state == "D3cold" {
            return "nvsleepify-gpu-suspended".into();
        }
        if state.power_state == "NotFound" {
            return "nvsleepify-gpu-off".into();
        }
        if state.mode == Mode::Integrated {
            return "nvsleepify-gpu-off".into();
        }
        "nvsleepify-gpu-active".into()
    }

    fn title_for_state(state: &UiState) -> String {
        if !state.processes.is_empty() {
            format!("GPU Active ({} proc)", state.processes.len())
        } else if state.power_state == "D3cold" {
            "GPU Suspended (D3cold)".into()
        } else {
            format!("nvsleepify ({})", state.mode)
        }
    }

    fn tooltip_for_state(state: &UiState) -> ksni::ToolTip {
        let mut lines = Vec::new();
        lines.push(format!("Mode: {}", state.mode));

        if !state.power_state.is_empty() && state.power_state != "NotFound" {
            lines.push(format!("Power: {}", state.power_state));
        }
        if !state.processes.is_empty() {
            lines.push("Processes using GPU:".into());
            for (name, pid) in &state.processes {
                lines.push(format!("- {} (PID {})", name, pid));
            }
        }
        if let Some(err) = &state.last_error {
            lines.push(format!("Error: {}", err));
        }

        ksni::ToolTip {
            title: "nvsleepify".into(),
            description: lines.join("\n"),
            ..Default::default()
        }
    }
}

impl ksni::Tray for NvSleepifyTray {
    fn id(&self) -> String {
        "nvsleepify-tray".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::Hardware
    }

    fn title(&self) -> String {
        Self::title_for_state(&self.state)
    }

    fn status(&self) -> ksni::Status {
        ksni::Status::Active
    }

    fn icon_name(&self) -> String {
        Self::icon_name_for_state(&self.state)
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        Self::tooltip_for_state(&self.state)
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;

        vec![
            CheckmarkItem {
                label: "Standard (Always On)".into(),
                checked: self.state.mode == Mode::Standard,
                activate: {
                    let tx = self.tx.clone();
                    Box::new(move |_| {
                        let _ = tx.send(TrayCommand::SetMode(Mode::Standard));
                    })
                },
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Integrated (Force Sleep)".into(),
                checked: self.state.mode == Mode::Integrated,
                activate: {
                    let tx = self.tx.clone();
                    Box::new(move |_| {
                        let _ = tx.send(TrayCommand::SetMode(Mode::Integrated));
                    })
                },
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Optimized (Auto)".into(),
                checked: self.state.mode == Mode::Optimized,
                activate: {
                    let tx = self.tx.clone();
                    Box::new(move |_| {
                        let _ = tx.send(TrayCommand::SetMode(Mode::Optimized));
                    })
                },
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            CheckmarkItem {
                label: "Notifications".into(),
                checked: self.notifications_enabled.load(Ordering::Relaxed),
                activate: {
                    let tx = self.tx.clone();
                    Box::new(move |_| {
                        let _ = tx.send(TrayCommand::ToggleNotifications);
                    })
                },
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: {
                    let tx = self.tx.clone();
                    Box::new(move |_| {
                        let _ = tx.send(TrayCommand::Quit);
                    })
                },
                ..Default::default()
            }
            .into(),
        ]
    }
}

fn confirm_kill_processes(procs: &[(String, String)]) -> bool {
    if procs.is_empty() {
        return true;
    }

    let mut text = String::new();
    text.push_str("The following processes are using the Nvidia GPU and may need to be killed to sleep it:\n\n");
    for (name, pid) in procs {
        text.push_str(&format!("- {} (PID {})\n", name, pid));
    }

    let result = rfd::MessageDialog::new()
        .set_title("nvsleepify")
        .set_description(&text)
        .set_buttons(rfd::MessageButtons::YesNo)
        .show();

    matches!(result, rfd::MessageDialogResult::Yes)
}

fn is_gpu_driver_loaded() -> bool {
    if let Ok(content) = std::fs::read_to_string("/proc/modules") {
        return content.lines().any(|l| l.starts_with("nvidia "));
    }
    false
}

async fn fetch_info(proxy: &NvSleepifyManagerProxy<'_>) -> UiState {
    match proxy.info().await {
        Ok((mode_str, power_state, processes)) => {
            let mode = Mode::from_str(&mode_str).unwrap_or(Mode::Standard);
            UiState {
                mode,
                power_state,
                processes,
                last_error: None,
            }
        }
        Err(e) => UiState {
            last_error: Some(format!(
                "Failed to query daemon: {} (is nvsleepifyd.service running?)",
                e
            )),
            ..Default::default()
        },
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let connection = Connection::system()
        .await
        .map_err(|e| anyhow!("Failed to connect to system bus: {}. Is dbus running?", e))?;

    let proxy = NvSleepifyManagerProxy::new(&connection).await.map_err(|e| {
        anyhow!(
            "Failed to connect to nvsleepify daemon at org.nvsleepify.Service: {}. Is nvsleepifyd.service running?",
            e
        )
    })?;

    let (tx, mut rx) = mpsc::unbounded_channel::<TrayCommand>();

    let initial_state = fetch_info(&proxy).await;
    let notifications_enabled = Arc::new(AtomicBool::new(true));
    let tray = NvSleepifyTray {
        state: initial_state.clone(),
        notifications_enabled: notifications_enabled.clone(),
        tx,
    };

    let handle = tray
        .spawn()
        .await
        .map_err(|e| anyhow!("Tray spawn failed: {e}"))?;

    // Polling logic
    {
        let handle = handle.clone();
        let proxy = NvSleepifyManagerProxy::new(&connection).await?;
        let notifications_enabled = notifications_enabled.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
            let mut last_state = initial_state;
            loop {
                interval.tick().await;
                let new_state = fetch_info(&proxy).await;

                if notifications_enabled.load(Ordering::Relaxed) {
                    if last_state.power_state != "D0" && new_state.power_state == "D0" {
                        if is_gpu_driver_loaded() {
                            tokio::task::spawn_blocking(|| {
                                let _ = Notification::new()
                                    .summary("nvsleepify")
                                    .body("GPU Woke up (D0)")
                                    .show();
                            });
                        }
                    }
                    if last_state.power_state != "D3cold" && new_state.power_state == "D3cold" {
                        tokio::task::spawn_blocking(|| {
                            let _ = Notification::new()
                                .summary("nvsleepify")
                                .body("GPU Suspended (D3cold)")
                                .show();
                        });
                    }
                    if last_state.mode != new_state.mode {
                        tokio::task::spawn_blocking(move || {
                            let _ = Notification::new()
                                .summary("nvsleepify")
                                .body(&format!("Mode changed to {}", new_state.mode))
                                .show();
                        });
                    }
                }

                last_state = new_state.clone();
                let _ = handle
                    .update(move |tray: &mut NvSleepifyTray| {
                        tray.state = new_state;
                    })
                    .await;
            }
        });
    }

    // Command handler
    {
        let handle = handle.clone();
        let proxy = NvSleepifyManagerProxy::new(&connection).await?;
        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    TrayCommand::ToggleNotifications => {
                        let current = notifications_enabled.load(Ordering::Relaxed);
                        notifications_enabled.store(!current, Ordering::Relaxed);
                        let _ = handle.update(|_| {}).await;
                    }
                    TrayCommand::Quit => {
                        let _ = handle.shutdown().await;
                        std::process::exit(0);
                    }
                    TrayCommand::SetMode(mode) => {
                        // Check blocking procs for Integrated or Optimized mode?
                        // If we are setting mode to Integrated, and there are processes, we might want to warn.
                        // But if we trust the daemon to force kill (Integrated), maybe we should warn first.
                        // The daemon's current logic for `set_mode` Integrated is `sleep_logic(true)` which kills.

                        let current = fetch_info(&proxy).await;
                        if mode == Mode::Integrated && !current.processes.is_empty() {
                            if !confirm_kill_processes(&current.processes) {
                                continue; // User cancelled
                            }
                        }

                        // What about Optimized? Daemon uses soft sleep.
                        // If user switches to Optimized and it fails to sleep due to processes,
                        // the daemon returns failure but stays in Optimized mode (and will retry in loop).
                        // That seems fine.

                        match proxy.set_mode(mode.to_string()).await {
                            Ok((success, msg, _procs)) => {
                                if !success {
                                    let _ = handle
                                        .update(|tray: &mut NvSleepifyTray| {
                                            tray.state.last_error =
                                                Some(format!("Set Mode failed: {}", msg));
                                        })
                                        .await;
                                }
                            }
                            Err(e) => {
                                let _ = handle
                                    .update(|tray: &mut NvSleepifyTray| {
                                        tray.state.last_error =
                                            Some(format!("Set Mode failed: {}", e));
                                    })
                                    .await;
                            }
                        }

                        let refreshed = fetch_info(&proxy).await;
                        let _ = handle
                            .update(|tray: &mut NvSleepifyTray| {
                                tray.state = refreshed;
                            })
                            .await;
                    }
                }
            }
        });
    }

    std::future::pending::<()>().await;
    Ok(())
}
