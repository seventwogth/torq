use std::sync::Arc;

use anyhow::{anyhow, Result};
use tokio::sync::{broadcast, mpsc, watch};
use torq_core::{TorCommand, TorEvent, TorState};

use crate::config::TorRuntimeConfig;
use crate::control::{TorControlClient, TorControlError};
use crate::logs::LogTail;
use crate::process::TorProcess;
use crate::runtime_events::{RuntimeEventSender, RUNTIME_EVENT_QUEUE_CAPACITY};
use crate::state;

pub struct TorManager {
    config: Arc<TorRuntimeConfig>,
    command_tx: mpsc::Sender<TorCommand>,
    state_rx: watch::Receiver<TorState>,
    event_tx: broadcast::Sender<TorEvent>,
}

struct RuntimeSession {
    process: TorProcess,
    logs: LogTail,
}

impl TorManager {
    pub async fn new(config: TorRuntimeConfig) -> Result<Self> {
        let config = Arc::new(config);
        let (command_tx, command_rx) = mpsc::channel(32);
        let (runtime_event_tx, runtime_event_rx) =
            RuntimeEventSender::channel(RUNTIME_EVENT_QUEUE_CAPACITY);
        let (state_tx, state_rx) = watch::channel(TorState::default());
        let (event_tx, _) = broadcast::channel(256);

        tokio::spawn(run_state_store(
            runtime_event_rx,
            state_tx,
            event_tx.clone(),
        ));
        tokio::spawn(run_supervisor(config.clone(), command_rx, runtime_event_tx));

        Ok(Self {
            config,
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

    pub async fn connect_control(&self) -> std::result::Result<TorControlClient, TorControlError> {
        let Some(control) = self.config.control.clone() else {
            return Err(TorControlError::MissingConfig);
        };

        let mut client = TorControlClient::new(control);
        client.connect().await?;
        Ok(client)
    }
}

async fn run_supervisor(
    config: Arc<TorRuntimeConfig>,
    mut command_rx: mpsc::Receiver<TorCommand>,
    runtime_event_tx: RuntimeEventSender,
) {
    let mut session: Option<RuntimeSession> = None;

    loop {
        if session.is_some() {
            let next_action = {
                let active_session = session.as_mut().expect("runtime session exists");
                tokio::select! {
                    command = command_rx.recv() => SupervisorAction::Command(command),
                    wait_result = active_session.process.wait() => SupervisorAction::ProcessExited(wait_result),
                }
            };

            match next_action {
                SupervisorAction::Command(Some(command)) => {
                    handle_command(command, config.as_ref(), &mut session, &runtime_event_tx).await;
                }
                SupervisorAction::Command(None) => {
                    if let Some(mut active_session) = session.take() {
                        let _ = stop_session(
                            &mut active_session,
                            config.as_ref(),
                            &runtime_event_tx,
                            true,
                        )
                        .await;
                    }
                    break;
                }
                SupervisorAction::ProcessExited(wait_result) => {
                    if let Some(mut active_session) = session.take() {
                        active_session.logs.stop().await;
                    }

                    publish_exit_event(wait_result, &runtime_event_tx).await;
                }
            }
        } else {
            let Some(command) = command_rx.recv().await else {
                break;
            };

            handle_command(command, config.as_ref(), &mut session, &runtime_event_tx).await;
        }
    }
}

async fn handle_command(
    command: TorCommand,
    config: &TorRuntimeConfig,
    session: &mut Option<RuntimeSession>,
    runtime_event_tx: &RuntimeEventSender,
) {
    match command {
        TorCommand::Start => {
            if session.is_some() {
                emit_event(
                    runtime_event_tx,
                    TorEvent::Warning("tor is already running".to_string()),
                )
                .await;
                return;
            }

            match start_session(config, runtime_event_tx).await {
                Ok(active_session) => {
                    *session = Some(active_session);
                    emit_event(runtime_event_tx, TorEvent::Started).await;
                }
                Err(error) => {
                    emit_event(runtime_event_tx, TorEvent::StartFailed(error.to_string())).await;
                }
            }
        }
        TorCommand::Stop => {
            let Some(mut active_session) = session.take() else {
                emit_event(
                    runtime_event_tx,
                    TorEvent::Warning("tor is not running".to_string()),
                )
                .await;
                return;
            };

            if let Err(error) =
                stop_session(&mut active_session, config, runtime_event_tx, true).await
            {
                emit_event(runtime_event_tx, TorEvent::Error(error.to_string())).await;
                *session = Some(active_session);
            }
        }
        TorCommand::Restart => {
            if let Some(mut active_session) = session.take() {
                if let Err(error) =
                    stop_session(&mut active_session, config, runtime_event_tx, true).await
                {
                    emit_event(runtime_event_tx, TorEvent::Error(error.to_string())).await;
                    *session = Some(active_session);
                    return;
                }
            }

            match start_session(config, runtime_event_tx).await {
                Ok(active_session) => {
                    *session = Some(active_session);
                    emit_event(runtime_event_tx, TorEvent::Started).await;
                }
                Err(error) => {
                    emit_event(runtime_event_tx, TorEvent::StartFailed(error.to_string())).await;
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
            )
            .await;
        }
    }
}

async fn start_session(
    config: &TorRuntimeConfig,
    runtime_event_tx: &RuntimeEventSender,
) -> Result<RuntimeSession> {
    let mut logs = LogTail::spawn(
        config.log_path.clone(),
        config.log_poll_interval,
        runtime_event_tx.clone(),
    )
    .await?;

    match TorProcess::spawn(config).await {
        Ok(process) => Ok(RuntimeSession { process, logs }),
        Err(error) => {
            logs.stop().await;
            Err(error)
        }
    }
}

async fn stop_session(
    session: &mut RuntimeSession,
    config: &TorRuntimeConfig,
    runtime_event_tx: &RuntimeEventSender,
    emit_stopped: bool,
) -> Result<()> {
    session.process.stop(config.stop_timeout).await?;
    session.logs.stop().await;

    if emit_stopped {
        emit_event(runtime_event_tx, TorEvent::Stopped).await;
    }

    Ok(())
}

async fn publish_exit_event(
    wait_result: std::io::Result<std::process::ExitStatus>,
    runtime_event_tx: &RuntimeEventSender,
) {
    match wait_result {
        Ok(status) if status.success() => {
            emit_event(runtime_event_tx, TorEvent::Stopped).await;
        }
        Ok(status) => {
            emit_event(
                runtime_event_tx,
                TorEvent::Crashed(format!("tor exited unexpectedly with status: {status}")),
            )
            .await;
        }
        Err(error) => {
            emit_event(
                runtime_event_tx,
                TorEvent::Crashed(format!("failed to wait for tor process: {error}")),
            )
            .await;
        }
    }
}

async fn run_state_store(
    mut runtime_event_rx: mpsc::Receiver<TorEvent>,
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

async fn emit_event(event_tx: &RuntimeEventSender, event: TorEvent) {
    event_tx.send(event).await;
}

enum SupervisorAction {
    Command(Option<TorCommand>),
    ProcessExited(std::io::Result<std::process::ExitStatus>),
}
