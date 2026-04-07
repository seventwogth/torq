mod desktop_shell;

use crate::desktop_shell::{
    get_runtime_config, set_runtime_config, tor_new_identity, tor_restart, tor_runtime_snapshot,
    tor_start, tor_state, tor_stop,
};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let state = tauri::async_runtime::block_on(desktop_shell::build_state());
            app.manage(state);
            desktop_shell::install_event_bridge(app);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            tor_state,
            tor_runtime_snapshot,
            get_runtime_config,
            set_runtime_config,
            tor_start,
            tor_stop,
            tor_restart,
            tor_new_identity
        ])
        .run(tauri::generate_context!())
        .expect("error while running torq desktop application");
}
