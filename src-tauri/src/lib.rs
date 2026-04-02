use std::env;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{Emitter, Manager, State};
use tokio::sync::broadcast;
use torq_core::{ControlAvailability, RuntimeStatus, TorEvent, TorState};
use torq_runtime::{TorManager, TorRuntimeSnapshot};

const TOR_STATE_EVENT: &str = "tor://state";
const TOR_RUNTIME_SNAPSHOT_EVENT: &str = "tor://runtime-snapshot";
const TOR_RUNTIME_ACTIVITY_EVENT: &str = "tor://activity";

/// Thin application state that keeps one shared `TorManager` alive for the app.
///
/// Tauri stores this once at startup so every command reads the same backend
/// instance. That keeps runtime ownership in the backend layer instead of
/// reconstructing supervisors or control clients per request.
pub struct AppState {
    manager: Option<TorManager>,
    init_error: Option<String>,
}

impl AppState {
    fn manager(&self) -> Result<&TorManager, String> {
        self.manager.as_ref().ok_or_else(|| {
            self.init_error
                .clone()
                .unwrap_or_else(|| "desktop backend is unavailable".to_string())
        })
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeActivityToneView {
    Neutral,
    Success,
    Warning,
    Danger,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeActivityKindView {
    Lifecycle,
    Bootstrap,
    Control,
    Identity,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TorRuntimeActivityView {
    pub id: u64,
    pub timestamp_ms: u128,
    pub kind: RuntimeActivityKindView,
    pub tone: RuntimeActivityToneView,
    pub title: String,
    pub details: Option<String>,
    pub coalesce_key: Option<String>,
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

impl TorRuntimeActivityView {
    fn new(
        id: u64,
        kind: RuntimeActivityKindView,
        tone: RuntimeActivityToneView,
        title: impl Into<String>,
        details: Option<String>,
        coalesce_key: Option<String>,
    ) -> Self {
        Self {
            id,
            timestamp_ms: current_timestamp_ms(),
            kind,
            tone,
            title: title.into(),
            details,
            coalesce_key,
        }
    }
}

fn current_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis())
}

fn runtime_activity_from_event(event: &TorEvent, id: u64) -> Option<TorRuntimeActivityView> {
    match event {
        TorEvent::Started => Some(TorRuntimeActivityView::new(
            id,
            RuntimeActivityKindView::Lifecycle,
            RuntimeActivityToneView::Success,
            "Tor started",
            None,
            None::<String>,
        )),
        TorEvent::Stopped => Some(TorRuntimeActivityView::new(
            id,
            RuntimeActivityKindView::Lifecycle,
            RuntimeActivityToneView::Neutral,
            "Tor stopped",
            None,
            None::<String>,
        )),
        TorEvent::IdentityRenewed => Some(TorRuntimeActivityView::new(
            id,
            RuntimeActivityKindView::Identity,
            RuntimeActivityToneView::Success,
            "New identity requested",
            None,
            None::<String>,
        )),
        TorEvent::StartFailed(message) => Some(TorRuntimeActivityView::new(
            id,
            RuntimeActivityKindView::Error,
            RuntimeActivityToneView::Danger,
            "Tor failed to start",
            Some(message.clone()),
            None::<String>,
        )),
        TorEvent::Crashed(message) => Some(TorRuntimeActivityView::new(
            id,
            RuntimeActivityKindView::Error,
            RuntimeActivityToneView::Danger,
            "Tor crashed",
            Some(message.clone()),
            None::<String>,
        )),
        TorEvent::Bootstrap(progress) => Some(TorRuntimeActivityView::new(
            id,
            RuntimeActivityKindView::Bootstrap,
            if *progress == 100 {
                RuntimeActivityToneView::Success
            } else {
                RuntimeActivityToneView::Info
            },
            format!("Bootstrap: {progress}%"),
            None,
            Some("bootstrap".to_string()),
        )),
        TorEvent::ControlAvailabilityChanged(availability) => {
            let (title, tone) = match availability {
                ControlAvailability::Available => (
                    "ControlPort became available",
                    RuntimeActivityToneView::Success,
                ),
                ControlAvailability::Unavailable => (
                    "ControlPort became unavailable",
                    RuntimeActivityToneView::Warning,
                ),
                ControlAvailability::Unconfigured => (
                    "ControlPort is unconfigured",
                    RuntimeActivityToneView::Neutral,
                ),
            };

            Some(TorRuntimeActivityView::new(
                id,
                RuntimeActivityKindView::Control,
                tone,
                title,
                None,
                None::<String>,
            ))
        }
        TorEvent::BootstrapObservationAvailabilityChanged(availability) => {
            let (title, tone) = match availability {
                ControlAvailability::Available => (
                    "Bootstrap observation became available",
                    RuntimeActivityToneView::Success,
                ),
                ControlAvailability::Unavailable => (
                    "Bootstrap observation became unavailable",
                    RuntimeActivityToneView::Warning,
                ),
                ControlAvailability::Unconfigured => (
                    "Bootstrap observation is unconfigured",
                    RuntimeActivityToneView::Neutral,
                ),
            };

            Some(TorRuntimeActivityView::new(
                id,
                RuntimeActivityKindView::Control,
                tone,
                title,
                None,
                None::<String>,
            ))
        }
        TorEvent::Warning(message) => Some(TorRuntimeActivityView::new(
            id,
            RuntimeActivityKindView::Warning,
            RuntimeActivityToneView::Warning,
            "Runtime warning",
            Some(message.clone()),
            None::<String>,
        )),
        TorEvent::Error(message) => Some(TorRuntimeActivityView::new(
            id,
            RuntimeActivityKindView::Error,
            RuntimeActivityToneView::Danger,
            "Runtime error",
            Some(message.clone()),
            None::<String>,
        )),
        TorEvent::LogLine(_) => None,
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

fn format_backend_init_error(error: &str) -> String {
    format!("Desktop backend could not initialize. {error}")
}

async fn build_state() -> AppState {
    match TorManager::new(default_runtime_config()).await {
        Ok(manager) => AppState {
            manager: Some(manager),
            init_error: None,
        },
        Err(error) => AppState {
            manager: None,
            init_error: Some(format_backend_init_error(&error.to_string())),
        },
    }
}

#[tauri::command]
fn tor_state(state: State<'_, AppState>) -> Result<TorStateView, String> {
    // Frontend commands return DTOs so the shell stays decoupled from the
    // runtime's internal watch channels and invariant-bearing backend types.
    Ok(state.manager()?.current_state().into())
}

#[tauri::command]
fn tor_runtime_snapshot(state: State<'_, AppState>) -> Result<TorRuntimeSnapshotView, String> {
    Ok(state.manager()?.current_runtime_state().into())
}

#[tauri::command]
async fn tor_start(state: State<'_, AppState>) -> Result<(), String> {
    let manager = state.manager()?;
    manager.start().await.map_err(|error| error.to_string())
}

#[tauri::command]
async fn tor_stop(state: State<'_, AppState>) -> Result<(), String> {
    let manager = state.manager()?;
    manager.stop().await.map_err(|error| error.to_string())
}

#[tauri::command]
async fn tor_restart(state: State<'_, AppState>) -> Result<(), String> {
    let manager = state.manager()?;
    manager.restart().await.map_err(|error| error.to_string())
}

#[tauri::command]
async fn tor_new_identity(state: State<'_, AppState>) -> Result<(), String> {
    let manager = state.manager()?;
    manager
        .new_identity()
        .await
        .map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let state = tauri::async_runtime::block_on(build_state());
            let app_handle = app.handle().clone();
            app.manage(state);
            let app_state = app.state::<AppState>();
            let Some(manager) = app_state.manager.as_ref() else {
                return Ok(());
            };
            let state_rx = manager.state_receiver();
            let runtime_state_rx = manager.runtime_state_receiver();
            let mut activity_rx = manager.subscribe_events();

            tauri::async_runtime::spawn({
                let app_handle = app_handle.clone();
                async move {
                    let mut state_rx = state_rx;

                    loop {
                        if state_rx.changed().await.is_err() {
                            return;
                        }

                        let state = *state_rx.borrow_and_update();
                        let _ = app_handle.emit(TOR_STATE_EVENT, TorStateView::from(state));
                    }
                }
            });

            tauri::async_runtime::spawn({
                let app_handle = app_handle.clone();
                async move {
                    let mut next_id = 0_u64;

                    loop {
                        match activity_rx.recv().await {
                            Ok(event) => {
                                next_id = next_id.saturating_add(1);
                                if let Some(activity) = runtime_activity_from_event(&event, next_id)
                                {
                                    let _ = app_handle.emit(TOR_RUNTIME_ACTIVITY_EVENT, activity);
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(broadcast::error::RecvError::Closed) => return,
                        }
                    }
                }
            });

            tauri::async_runtime::spawn(async move {
                let mut runtime_state_rx = runtime_state_rx;

                loop {
                    if runtime_state_rx.changed().await.is_err() {
                        return;
                    }

                    let runtime_state = *runtime_state_rx.borrow_and_update();
                    let _ = app_handle.emit(
                        TOR_RUNTIME_SNAPSHOT_EVENT,
                        TorRuntimeSnapshotView::from(runtime_state),
                    );
                }
            });
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
