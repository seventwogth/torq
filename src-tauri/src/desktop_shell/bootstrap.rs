use std::env;
use std::path::PathBuf;

use torq_runtime::{TorManager, TorRuntimeConfig};

use super::config_store::RuntimeConfigStore;
use super::state::AppState;

pub async fn build_state() -> AppState {
    let config_store = match build_config_store() {
        Ok(config_store) => config_store,
        Err(error) => return AppState::new(None, None, Some(error)),
    };

    match build_manager(&config_store).await {
        Ok(manager) => AppState::new(Some(manager), Some(config_store), None),
        Err(error) => AppState::new(None, Some(config_store), Some(error)),
    }
}

fn user_config_root() -> Option<PathBuf> {
    env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
}

fn runtime_config_path() -> PathBuf {
    if let Some(root) = user_config_root() {
        return root.join("torq").join("torq.config.json");
    }

    env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("torq.config.json")
}

fn build_config_store() -> Result<RuntimeConfigStore, String> {
    RuntimeConfigStore::new(runtime_config_path(), default_runtime_config()).map_err(|error| {
        format_backend_init_error(&format!("failed to load runtime config. {error}"))
    })
}

async fn build_manager(config_store: &RuntimeConfigStore) -> Result<TorManager, String> {
    let config = config_store.load().map_err(|error| {
        format_backend_init_error(&format!("failed to load runtime config. {error}"))
    })?;

    TorManager::new(config)
        .await
        .map_err(|error| format_backend_init_error(&error.to_string()))
}

fn default_runtime_config() -> TorRuntimeConfig {
    let tor_path = env::var_os("TORQ_TOR_EXE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tor.exe"));
    let log_path = env::var_os("TORQ_TOR_LOG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tor.log"));

    TorRuntimeConfig::new(tor_path, log_path)
}

fn format_backend_init_error(error: &str) -> String {
    format!("Desktop backend could not initialize. {error}")
}
