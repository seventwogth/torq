use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout, Duration};
use torq_core::{ControlAvailability, TorEvent};

use crate::control::{TorBootstrapPhase, TorControlClient, TorControlConfig};
use crate::runtime_events::RuntimeEventSender;

use super::state_store::emit_event;

const CONTROL_BOOTSTRAP_POLL_INTERVAL: Duration = Duration::from_millis(250);
const AUX_TASK_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);

pub(super) struct BootstrapObserver {
    shutdown_tx: Option<watch::Sender<bool>>,
    task: Option<JoinHandle<()>>,
}

impl BootstrapObserver {
    pub(super) fn spawn(config: TorControlConfig, event_tx: RuntimeEventSender) -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let task = tokio::spawn(run_bootstrap_observer(config, event_tx, shutdown_rx));

        Self {
            shutdown_tx: Some(shutdown_tx),
            task: Some(task),
        }
    }

    pub(super) fn begin_stop(&mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(true);
        }
    }

    pub(super) async fn finish_stop(&mut self) {
        if let Some(mut task) = self.task.take() {
            match timeout(AUX_TASK_SHUTDOWN_TIMEOUT, &mut task).await {
                Ok(join_result) => {
                    let _ = join_result;
                }
                Err(_) => {
                    task.abort();
                    let _ = task.await;
                }
            }
        }
    }

    pub(super) async fn stop(&mut self) {
        self.begin_stop();
        self.finish_stop().await;
    }
}

async fn run_bootstrap_observer(
    control_config: TorControlConfig,
    runtime_event_tx: RuntimeEventSender,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    // ControlPort is now the preferred operational bootstrap source when it is
    // configured. Log-derived bootstrap events still flow through the same
    // reducer as a fallback/diagnostic channel, so this task only adds
    // authoritative snapshots and does not replace the log tail entirely yet.
    let mut client = TorControlClient::new(control_config.clone());
    let mut last_progress = None;
    let mut last_warning = None;
    let mut last_control_availability = ControlAvailability::Unavailable;
    let mut last_observer_availability = ControlAvailability::Unavailable;

    loop {
        let poll_result = tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    return;
                }
                continue;
            }
            result = client.get_bootstrap_phase() => result,
        };

        match poll_result {
            Ok(phase) => {
                last_warning = None;
                if last_control_availability != ControlAvailability::Available {
                    emit_event(
                        &runtime_event_tx,
                        TorEvent::ControlAvailabilityChanged(ControlAvailability::Available),
                    )
                    .await;
                    last_control_availability = ControlAvailability::Available;
                }
                if last_observer_availability != ControlAvailability::Available {
                    emit_event(
                        &runtime_event_tx,
                        TorEvent::BootstrapObservationAvailabilityChanged(
                            ControlAvailability::Available,
                        ),
                    )
                    .await;
                    last_observer_availability = ControlAvailability::Available;
                }

                if let Some(event) = bootstrap_phase_event(last_progress, &phase) {
                    last_progress = Some(phase.progress());
                    emit_event(&runtime_event_tx, event).await;
                }

                if phase.progress() == 100 {
                    return;
                }
            }
            Err(error) => {
                let warning =
                    format!("bootstrap observation via ControlPort is unavailable: {error}");

                if last_control_availability != ControlAvailability::Unavailable {
                    emit_event(
                        &runtime_event_tx,
                        TorEvent::ControlAvailabilityChanged(ControlAvailability::Unavailable),
                    )
                    .await;
                    last_control_availability = ControlAvailability::Unavailable;
                }
                if last_observer_availability != ControlAvailability::Unavailable {
                    emit_event(
                        &runtime_event_tx,
                        TorEvent::BootstrapObservationAvailabilityChanged(
                            ControlAvailability::Unavailable,
                        ),
                    )
                    .await;
                    last_observer_availability = ControlAvailability::Unavailable;
                }
                if last_warning.as_deref() != Some(warning.as_str()) {
                    emit_event(&runtime_event_tx, TorEvent::Warning(warning.clone())).await;
                    last_warning = Some(warning);
                }

                // A failed poll can leave the underlying TCP session stale.
                // Rebuild the narrow client on the next iteration instead of
                // trying to recover with a larger connection manager.
                client = TorControlClient::new(control_config.clone());
            }
        }

        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    return;
                }
            }
            _ = sleep(CONTROL_BOOTSTRAP_POLL_INTERVAL) => {}
        }
    }
}

pub(super) fn bootstrap_phase_event(
    last_progress: Option<u8>,
    phase: &TorBootstrapPhase,
) -> Option<TorEvent> {
    (last_progress.is_none_or(|previous| phase.progress() > previous))
        .then_some(TorEvent::Bootstrap(phase.progress()))
}
