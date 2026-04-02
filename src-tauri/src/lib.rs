use std::env;
use std::path::PathBuf;

use serde::Serialize;
use tauri::{Manager, State};
use torq_core::{ControlAvailability, RuntimeStatus, TorState};
use torq_runtime::{TorManager, TorRuntimeSnapshot};

/// Thin application state that keeps one shared `TorManager` alive for the app.
///
/// Tauri stores this once at startup so every command reads the same backend
/// instance. That keeps runtime ownership in the backend layer instead of
/// reconstructing supervisors or control clients per request.
pub struct AppState {
    manager: TorManager,
}

impl AppState {
    fn manager(&self) -> &TorManager {
        &self.manager
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStatusView {
    Stopped,
    Starting,
    Running,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlAvailabilityView {
    Unconfigured,
    Unavailable,
    Available,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TorStateView {
    pub status: RuntimeStatusView,
    pub bootstrap: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TorControlRuntimeView {
    pub port: ControlAvailabilityView,
    pub bootstrap_observation: ControlAvailabilityView,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TorRuntimeSnapshotView {
    pub tor: TorStateView,
    pub control: TorControlRuntimeView,
    pub control_configured: bool,
    pub control_available: bool,
    pub bootstrap_observation_available: bool,
    pub new_identity_available: bool,
    pub uses_control_bootstrap_observation: bool,
}

impl From<RuntimeStatus> for RuntimeStatusView {
    fn from(value: RuntimeStatus) -> Self {
        match value {
            RuntimeStatus::Stopped => Self::Stopped,
            RuntimeStatus::Starting => Self::Starting,
            RuntimeStatus::Running => Self::Running,
            RuntimeStatus::Failed => Self::Failed,
        }
    }
}

impl From<ControlAvailability> for ControlAvailabilityView {
    fn from(value: ControlAvailability) -> Self {
        match value {
            ControlAvailability::Unconfigured => Self::Unconfigured,
            ControlAvailability::Unavailable => Self::Unavailable,
            ControlAvailability::Available => Self::Available,
        }
    }
}

impl From<TorState> for TorStateView {
    fn from(value: TorState) -> Self {
        Self {
            status: value.status().into(),
            bootstrap: value.bootstrap(),
        }
    }
}

impl From<torq_runtime::TorControlRuntime> for TorControlRuntimeView {
    fn from(value: torq_runtime::TorControlRuntime) -> Self {
        Self {
            port: value.port().into(),
            bootstrap_observation: value.bootstrap_observation().into(),
        }
    }
}

impl From<TorRuntimeSnapshot> for TorRuntimeSnapshotView {
    fn from(value: TorRuntimeSnapshot) -> Self {
        Self {
            tor: value.tor().into(),
            control: value.control().into(),
            control_configured: value.control_configured(),
            control_available: value.control_available(),
            bootstrap_observation_available: value.bootstrap_observation_available(),
            new_identity_available: value.new_identity_available(),
            uses_control_bootstrap_observation: value.uses_control_bootstrap_observation(),
        }
    }
}

fn default_runtime_config() -> torq_runtime::TorRuntimeConfig {
    // The first desktop shell only needs a stable manager bootstrap path.
    // Process launch controls stay out of the UI for now, so startup mirrors the
    // CLI defaults and can be overridden by environment variables when needed.
    let tor_path = env::var_os("TORQ_TOR_EXE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tor.exe"));
    let log_path = env::var_os("TORQ_TOR_LOG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tor.log"));

    torq_runtime::TorRuntimeConfig::new(tor_path, log_path)
}

async fn build_state() -> Result<AppState, String> {
    let manager = TorManager::new(default_runtime_config())
        .await
        .map_err(|error| error.to_string())?;

    Ok(AppState { manager })
}

#[tauri::command]
fn tor_state(state: State<'_, AppState>) -> TorStateView {
    // Frontend commands return DTOs so the shell stays decoupled from the
    // runtime's internal watch channels and invariant-bearing backend types.
    state.manager().current_state().into()
}

#[tauri::command]
fn tor_runtime_snapshot(state: State<'_, AppState>) -> TorRuntimeSnapshotView {
    state.manager().current_runtime_state().into()
}

#[tauri::command]
async fn tor_start(state: State<'_, AppState>) -> Result<(), String> {
    state
        .manager()
        .start()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn tor_stop(state: State<'_, AppState>) -> Result<(), String> {
    state
        .manager()
        .stop()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn tor_restart(state: State<'_, AppState>) -> Result<(), String> {
    state
        .manager()
        .restart()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn tor_new_identity(state: State<'_, AppState>) -> Result<(), String> {
    state
        .manager()
        .new_identity()
        .await
        .map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let state = tauri::async_runtime::block_on(build_state())
                .expect("failed to initialize tor runtime state");
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            tor_state,
            tor_runtime_snapshot,
            tor_start,
            tor_stop,
            tor_restart,
            tor_new_identity
        ])
        .run(tauri::generate_context!())
        .expect("error while running torq desktop application");
}
