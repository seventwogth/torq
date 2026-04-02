use anyhow::{anyhow, Result};
use tokio::sync::{broadcast, mpsc, watch};
use torq_core::{TorCommand, TorEvent, TorState};

use crate::config::TorRuntimeConfig;
use crate::control::TorControlClient;
use crate::logs::LogTail;
use crate::process::TorProcess;
use crate::runtime_events::{RuntimeEventSender, RUNTIME_EVENT_QUEUE_CAPACITY};
use crate::state;

pub struct TorManager {
    command_tx: mpsc::Sender<TorCommand>,
    state_rx: watch::Receiver<TorState>,
    event_tx: broadcast::Sender<TorEvent>,
}

struct RuntimeSession {
    process: TorProcess,
    logs: LogTail,
    // ControlPort stays session-owned so control actions follow the lifetime
    // of the running Tor instance rather than becoming manager-global state.
    control: Option<TorControlClient>,
}

impl TorManager {
    pub async fn new(config: TorRuntimeConfig) -> Result<Self> {
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
                    handle_command(command, &config, &mut session, &runtime_event_tx).await;
                }
                SupervisorAction::Command(None) => {
                    if let Some(mut active_session) = session.take() {
                        let _ = stop_session(&mut active_session, &config, &runtime_event_tx, true)
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

            handle_command(command, &config, &mut session, &runtime_event_tx).await;
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
            handle_new_identity(session, runtime_event_tx).await;
        }
    }
}

async fn handle_new_identity(
    session: &mut Option<RuntimeSession>,
    runtime_event_tx: &RuntimeEventSender,
) {
    let Some(active_session) = session.as_mut() else {
        emit_event(
            runtime_event_tx,
            TorEvent::Warning("tor is not running".to_string()),
        )
        .await;
        return;
    };

    let Some(control) = active_session.control.as_mut() else {
        emit_event(
            runtime_event_tx,
            TorEvent::Warning("new identity requires ControlPort configuration".to_string()),
        )
        .await;
        return;
    };

    match control.signal_newnym().await {
        Ok(_) => emit_event(runtime_event_tx, TorEvent::IdentityRenewed).await,
        Err(error) => {
            emit_event(
                runtime_event_tx,
                TorEvent::Error(format!(
                    "failed to request new identity via ControlPort: {error}"
                )),
            )
            .await;
        }
    }
}

async fn start_session(
    config: &TorRuntimeConfig,
    runtime_event_tx: &RuntimeEventSender,
) -> Result<RuntimeSession> {
    let control = config.control.clone().map(TorControlClient::new);
    let mut logs = LogTail::spawn(
        config.log_path.clone(),
        config.log_poll_interval,
        runtime_event_tx.clone(),
    )
    .await?;

    match TorProcess::spawn(config).await {
        Ok(process) => Ok(RuntimeSession {
            process,
            logs,
            control,
        }),
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::broadcast;
    use tokio::time::timeout;
    use torq_core::TorEvent;

    use super::TorManager;
    use crate::config::TorRuntimeConfig;
    use crate::control::{TorControlAuth, TorControlConfig};

    #[tokio::test]
    async fn new_identity_without_running_tor_emits_warning() {
        let manager = TorManager::new(TorRuntimeConfig::new(
            "tor.exe",
            unique_test_path("idle.log"),
        ))
        .await
        .unwrap();
        let mut events = manager.subscribe_events();

        manager.new_identity().await.unwrap();

        assert_eq!(
            recv_matching_event(&mut events, |event| matches!(
                event,
                TorEvent::Warning(message) if message == "tor is not running"
            ))
            .await,
            TorEvent::Warning("tor is not running".to_string())
        );
    }

    #[tokio::test]
    async fn new_identity_without_control_config_emits_warning() {
        let script = mock_tor_script_path();
        let log_path = unique_test_path("missing-control.log");
        let config = TorRuntimeConfig::new("cmd.exe", &log_path)
            .with_args(["/C", script.to_string_lossy().as_ref()])
            .with_stop_timeout(Duration::from_secs(1));
        let manager = TorManager::new(config).await.unwrap();
        let mut events = manager.subscribe_events();

        manager.start().await.unwrap();
        let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Started).await;

        manager.new_identity().await.unwrap();

        assert_eq!(
            recv_matching_event(&mut events, |event| matches!(
                event,
                TorEvent::Warning(message)
                    if message == "new identity requires ControlPort configuration"
            ))
            .await,
            TorEvent::Warning("new identity requires ControlPort configuration".to_string())
        );

        manager.stop().await.unwrap();
        let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Stopped).await;
        cleanup(&log_path).await;
    }

    #[tokio::test]
    async fn new_identity_success_emits_identity_renewed() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut reader = tokio::io::BufReader::new(socket);
            let mut line = String::new();

            reader.read_line(&mut line).await.unwrap();
            assert_eq!(line.trim_end_matches(['\r', '\n']), "AUTHENTICATE");
            reader.get_mut().write_all(b"250 OK\r\n").await.unwrap();

            line.clear();
            reader.read_line(&mut line).await.unwrap();
            assert_eq!(line.trim_end_matches(['\r', '\n']), "SIGNAL NEWNYM");
            reader.get_mut().write_all(b"250 OK\r\n").await.unwrap();
        });

        let script = mock_tor_script_path();
        let log_path = unique_test_path("identity-renewed.log");
        let control = TorControlConfig::new(
            address.ip().to_string(),
            address.port(),
            TorControlAuth::Null,
        );
        let config = TorRuntimeConfig::new("cmd.exe", &log_path)
            .with_args(["/C", script.to_string_lossy().as_ref()])
            .with_control(control)
            .with_stop_timeout(Duration::from_secs(1));
        let manager = TorManager::new(config).await.unwrap();
        let mut events = manager.subscribe_events();

        manager.start().await.unwrap();
        let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Started).await;

        manager.new_identity().await.unwrap();

        assert_eq!(
            recv_matching_event(&mut events, |event| *event == TorEvent::IdentityRenewed).await,
            TorEvent::IdentityRenewed
        );

        manager.stop().await.unwrap();
        let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Stopped).await;
        server.await.unwrap();
        cleanup(&log_path).await;
    }

    async fn recv_matching_event<F>(
        events: &mut broadcast::Receiver<TorEvent>,
        predicate: F,
    ) -> TorEvent
    where
        F: Fn(&TorEvent) -> bool,
    {
        timeout(Duration::from_secs(5), async {
            loop {
                match events.recv().await {
                    Ok(event) if predicate(&event) => return event,
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => panic!("event channel closed"),
                }
            }
        })
        .await
        .expect("timed out while waiting for matching runtime event")
    }

    fn mock_tor_script_path() -> PathBuf {
        workspace_root().join("scripts").join("mock-tor.cmd")
    }

    fn workspace_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .to_path_buf()
    }

    async fn cleanup(log_path: &Path) {
        let _ = tokio::fs::remove_file(log_path).await;
    }

    fn unique_test_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("torq-manager-{label}-{nonce}.log"))
    }
}
