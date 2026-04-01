use anyhow::{anyhow, Result};
use tokio::process::Child;
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinHandle;
use torq_core::{TorCommand, TorEvent, TorState};

use crate::config::TorRuntimeConfig;
use crate::logs;
use crate::process;
use crate::state;

pub struct TorManager {
    command_tx: mpsc::Sender<TorCommand>,
    state_rx: watch::Receiver<TorState>,
    event_tx: broadcast::Sender<TorEvent>,
}

struct ManagedRuntime {
    child: Child,
    log_task: JoinHandle<()>,
}

impl TorManager {
    pub async fn new(config: TorRuntimeConfig) -> Result<Self> {
        let (command_tx, command_rx) = mpsc::channel(32);
        let (runtime_event_tx, runtime_event_rx) = mpsc::unbounded_channel();
        let (state_tx, state_rx) = watch::channel(TorState::default());
        let (event_tx, _) = broadcast::channel(256);

        logs::prepare_log_file(&config.log_path).await?;

        tokio::spawn(run_state_store(
            runtime_event_rx,
            state_tx,
            event_tx.clone(),
        ));
        tokio::spawn(run_supervisor(config, command_rx, runtime_event_tx));

        Ok(Self {
            command_tx,
            state_rx,
            event_tx,
        })
    }

    pub fn command_sender(&self) -> mpsc::Sender<TorCommand> {
        self.command_tx.clone()
    }

    pub fn state_receiver(&self) -> watch::Receiver<TorState> {
        self.state_rx.clone()
    }

    pub fn state(&self) -> watch::Receiver<TorState> {
        self.state_rx.clone()
    }

    pub fn current_state(&self) -> TorState {
        *self.state_rx.borrow()
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<TorEvent> {
        self.event_tx.subscribe()
    }

    pub async fn send(&self, command: TorCommand) -> Result<()> {
        self.command_tx
            .send(command)
            .await
            .map_err(|_| anyhow!("tor runtime supervisor is not available"))
    }

    pub async fn send_command(&self, command: TorCommand) -> Result<()> {
        self.send(command).await
    }

    pub async fn start(&self) -> Result<()> {
        self.send(TorCommand::Start).await
    }

    pub async fn stop(&self) -> Result<()> {
        self.send(TorCommand::Stop).await
    }

    pub async fn restart(&self) -> Result<()> {
        self.send(TorCommand::Restart).await
    }

    pub async fn new_identity(&self) -> Result<()> {
        self.send(TorCommand::NewIdentity).await
    }
}

async fn run_supervisor(
    config: TorRuntimeConfig,
    mut command_rx: mpsc::Receiver<TorCommand>,
    runtime_event_tx: mpsc::UnboundedSender<TorEvent>,
) {
    let mut runtime: Option<ManagedRuntime> = None;

    loop {
        if runtime.is_some() {
            let next_action = {
                let active_runtime = runtime.as_mut().expect("runtime exists");
                tokio::select! {
                    command = command_rx.recv() => SupervisorAction::Command(command),
                    wait_result = process::wait_for_exit(&mut active_runtime.child) => SupervisorAction::ProcessExited(wait_result),
                }
            };

            match next_action {
                SupervisorAction::Command(Some(command)) => {
                    handle_command(command, &config, &mut runtime, &runtime_event_tx).await;
                }
                SupervisorAction::Command(None) => {
                    if let Some(active_runtime) = runtime.take() {
                        let _ = shutdown_runtime(&config, active_runtime, &runtime_event_tx, true)
                            .await;
                    }
                    break;
                }
                SupervisorAction::ProcessExited(wait_result) => {
                    if let Some(active_runtime) = runtime.take() {
                        active_runtime.log_task.abort();
                        let _ = active_runtime.log_task.await;
                    }

                    match wait_result {
                        Ok(status) if status.success() => {
                            emit_event(&runtime_event_tx, TorEvent::Stopped);
                        }
                        Ok(status) => {
                            emit_event(
                                &runtime_event_tx,
                                TorEvent::Crashed(format!(
                                    "tor exited unexpectedly with status: {status}"
                                )),
                            );
                        }
                        Err(error) => {
                            emit_event(
                                &runtime_event_tx,
                                TorEvent::Crashed(format!(
                                    "failed to wait for tor process: {error}"
                                )),
                            );
                        }
                    }
                }
            }
        } else {
            let Some(command) = command_rx.recv().await else {
                break;
            };

            handle_command(command, &config, &mut runtime, &runtime_event_tx).await;
        }
    }
}

async fn handle_command(
    command: TorCommand,
    config: &TorRuntimeConfig,
    runtime: &mut Option<ManagedRuntime>,
    runtime_event_tx: &mpsc::UnboundedSender<TorEvent>,
) {
    match command {
        TorCommand::Start => {
            if runtime.is_some() {
                emit_event(
                    runtime_event_tx,
                    TorEvent::Warning("tor is already running".to_string()),
                );
                return;
            }

            match start_runtime(config, runtime_event_tx).await {
                Ok(active_runtime) => {
                    *runtime = Some(active_runtime);
                    emit_event(runtime_event_tx, TorEvent::Started);
                }
                Err(error) => {
                    emit_event(runtime_event_tx, TorEvent::StartFailed(error.to_string()));
                }
            }
        }
        TorCommand::Stop => {
            let Some(active_runtime) = runtime.take() else {
                emit_event(
                    runtime_event_tx,
                    TorEvent::Warning("tor is not running".to_string()),
                );
                return;
            };

            if let Err(error) =
                shutdown_runtime(config, active_runtime, runtime_event_tx, true).await
            {
                emit_event(runtime_event_tx, TorEvent::Error(error.to_string()));
            }
        }
        TorCommand::Restart => {
            if let Some(active_runtime) = runtime.take() {
                if let Err(error) =
                    shutdown_runtime(config, active_runtime, runtime_event_tx, true).await
                {
                    emit_event(runtime_event_tx, TorEvent::Error(error.to_string()));
                    return;
                }
            }

            match start_runtime(config, runtime_event_tx).await {
                Ok(active_runtime) => {
                    *runtime = Some(active_runtime);
                    emit_event(runtime_event_tx, TorEvent::Started);
                }
                Err(error) => {
                    emit_event(runtime_event_tx, TorEvent::StartFailed(error.to_string()));
                }
            }
        }
        TorCommand::NewIdentity => {
            emit_event(
                runtime_event_tx,
                TorEvent::Warning(
                    "new identity requested, but ControlPort is not implemented in runtime MVP"
                        .to_string(),
                ),
            );
        }
    }
}

async fn start_runtime(
    config: &TorRuntimeConfig,
    runtime_event_tx: &mpsc::UnboundedSender<TorEvent>,
) -> Result<ManagedRuntime> {
    logs::prepare_log_file(&config.log_path).await?;

    let child = process::spawn_process(config).await?;
    let log_task = logs::spawn_log_task(
        config.log_path.clone(),
        config.log_poll_interval,
        runtime_event_tx.clone(),
    );

    Ok(ManagedRuntime { child, log_task })
}

async fn shutdown_runtime(
    config: &TorRuntimeConfig,
    mut runtime: ManagedRuntime,
    runtime_event_tx: &mpsc::UnboundedSender<TorEvent>,
    emit_stopped: bool,
) -> Result<()> {
    runtime.log_task.abort();
    let _ = runtime.log_task.await;

    process::stop_process(&mut runtime.child, config.stop_timeout).await?;

    if emit_stopped {
        emit_event(runtime_event_tx, TorEvent::Stopped);
    }

    Ok(())
}

async fn run_state_store(
    mut runtime_event_rx: mpsc::UnboundedReceiver<TorEvent>,
    state_tx: watch::Sender<TorState>,
    event_tx: broadcast::Sender<TorEvent>,
) {
    let mut current_state = TorState::default();

    while let Some(event) = runtime_event_rx.recv().await {
        let mut next_state = current_state;
        if state::apply_event(&mut next_state, &event).is_ok() && next_state != current_state {
            current_state = next_state;
            let _ = state_tx.send(current_state);
        }

        let _ = event_tx.send(event);
    }
}

fn emit_event(event_tx: &mpsc::UnboundedSender<TorEvent>, event: TorEvent) {
    let _ = event_tx.send(event);
}

enum SupervisorAction {
    Command(Option<TorCommand>),
    ProcessExited(std::io::Result<std::process::ExitStatus>),
}
