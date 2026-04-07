use tauri::State;

use super::runtime_config::{
    get_runtime_config_response, set_runtime_config_request, RuntimeConfigRequest,
    RuntimeConfigResponse,
};
use super::state::AppState;
use super::views::{TorRuntimeSnapshotView, TorStateView};

#[tauri::command]
pub fn tor_state(state: State<'_, AppState>) -> Result<TorStateView, String> {
    // Frontend commands return DTOs so the shell stays decoupled from the
    // runtime's internal watch channels and invariant-bearing backend types.
    Ok(state.manager()?.current_state().into())
}

#[tauri::command]
pub fn tor_runtime_snapshot(state: State<'_, AppState>) -> Result<TorRuntimeSnapshotView, String> {
    Ok(state.manager()?.current_runtime_state().into())
}

#[tauri::command]
pub fn get_runtime_config(state: State<'_, AppState>) -> Result<RuntimeConfigResponse, String> {
    let config = state
        .config_store()?
        .get()
        .map_err(|error| error.to_string())?;
    Ok(get_runtime_config_response(&config))
}

#[tauri::command]
pub async fn tor_start(state: State<'_, AppState>) -> Result<(), String> {
    let _guard = state.lock_commands().await;
    let manager = state.manager()?;
    manager.start().await.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn tor_stop(state: State<'_, AppState>) -> Result<(), String> {
    let _guard = state.lock_commands().await;
    let manager = state.manager()?;
    manager.stop().await.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn tor_restart(state: State<'_, AppState>) -> Result<(), String> {
    let _guard = state.lock_commands().await;
    let manager = state.manager()?;
    manager.restart().await.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn tor_new_identity(state: State<'_, AppState>) -> Result<(), String> {
    let _guard = state.lock_commands().await;
    let manager = state.manager()?;
    manager
        .new_identity()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn set_runtime_config(
    state: State<'_, AppState>,
    config: RuntimeConfigRequest,
) -> Result<RuntimeConfigResponse, String> {
    let _guard = state.lock_commands().await;
    let manager = state.manager()?;
    let config_store = state.config_store()?;
    let previous_config = config_store.get().map_err(|error| error.to_string())?;
    let next_config = set_runtime_config_request(config).map_err(|error| error.to_string())?;

    manager
        .set_runtime_config(next_config.clone())
        .await
        .map_err(|error| error.to_string())?;

    if let Err(error) = config_store.save(next_config.clone()) {
        let rollback_result = manager.set_runtime_config(previous_config.clone()).await;
        if rollback_result.is_ok() {
            let _ = config_store.set(previous_config);
        }

        return Err(match rollback_result {
            Ok(_) => format!("failed to persist runtime config. {error}"),
            Err(rollback_error) => format!(
                "failed to persist runtime config. {error}. runtime rollback also failed: {rollback_error}"
            ),
        });
    }

    config_store
        .set(next_config.clone())
        .map_err(|error| error.to_string())?;

    Ok(get_runtime_config_response(&next_config))
}
