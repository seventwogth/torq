use tauri::{App, Emitter, Manager, Runtime};
use tokio::sync::broadcast;

use super::activity::runtime_activity_from_event;
use super::state::AppState;
use super::views::{TorRuntimeSnapshotView, TorStateView};

const TOR_STATE_EVENT: &str = "tor://state";
const TOR_RUNTIME_SNAPSHOT_EVENT: &str = "tor://runtime-snapshot";
const TOR_RUNTIME_ACTIVITY_EVENT: &str = "tor://activity";

pub fn install<R: Runtime>(app: &mut App<R>) {
    let app_handle = app.handle().clone();
    let app_state = app.state::<AppState>();
    let Ok(manager) = app_state.manager() else {
        return;
    };
    let state_rx = manager.state();
    let runtime_state_rx = manager.runtime_state();
    let activity_rx = manager.subscribe_events();

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
            let mut activity_rx = activity_rx;
            let mut next_id = 0_u64;

            loop {
                match activity_rx.recv().await {
                    Ok(event) => {
                        next_id = next_id.saturating_add(1);
                        if let Some(activity) = runtime_activity_from_event(&event, next_id) {
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
}
