use tokio::sync::{broadcast, mpsc, watch};
use torq_core::{TorEvent, TorState};

use crate::runtime_events::RuntimeEventSender;
use crate::runtime_state::{TorRuntimeSnapshot, TorRuntimeSnapshotReducer};

pub(super) async fn run_state_store(
    mut runtime_event_rx: mpsc::Receiver<TorEvent>,
    state_tx: watch::Sender<TorState>,
    runtime_state_tx: watch::Sender<TorRuntimeSnapshot>,
    initial_runtime_state: TorRuntimeSnapshot,
    event_tx: broadcast::Sender<TorEvent>,
) {
    let mut current_state = initial_runtime_state;

    while let Some(event) = runtime_event_rx.recv().await {
        let mut next_state = current_state;
        if TorRuntimeSnapshotReducer::apply_event(&mut next_state, &event).is_ok()
            && next_state != current_state
        {
            let tor_changed = next_state.tor() != current_state.tor();
            current_state = next_state;

            if tor_changed {
                let _ = state_tx.send(current_state.tor());
            }

            let _ = runtime_state_tx.send(current_state);
        }

        let _ = event_tx.send(event);
    }
}

pub(super) async fn emit_event(event_tx: &RuntimeEventSender, event: TorEvent) {
    event_tx.send(event).await;
}
