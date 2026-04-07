use tokio::sync::{Mutex, MutexGuard};
use torq_runtime::TorManager;

use super::config_store::RuntimeConfigStore;

/// Shared desktop backend state registered once in Tauri.
pub struct AppState {
    manager: Option<TorManager>,
    config_store: Option<RuntimeConfigStore>,
    init_error: Option<String>,
    command_lock: Mutex<()>,
}

impl AppState {
    pub fn new(
        manager: Option<TorManager>,
        config_store: Option<RuntimeConfigStore>,
        init_error: Option<String>,
    ) -> Self {
        Self {
            manager,
            config_store,
            init_error,
            command_lock: Mutex::new(()),
        }
    }

    pub fn manager(&self) -> Result<&TorManager, String> {
        self.manager
            .as_ref()
            .ok_or_else(|| self.backend_error("desktop backend is unavailable"))
    }

    pub fn config_store(&self) -> Result<&RuntimeConfigStore, String> {
        self.config_store
            .as_ref()
            .ok_or_else(|| self.backend_error("runtime config store is unavailable"))
    }

    pub async fn lock_commands(&self) -> MutexGuard<'_, ()> {
        self.command_lock.lock().await
    }

    fn backend_error(&self, fallback: &str) -> String {
        self.init_error
            .clone()
            .unwrap_or_else(|| fallback.to_string())
    }
}
