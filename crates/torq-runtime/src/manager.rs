use anyhow::{anyhow, Result};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout, Duration};
use torq_core::{ControlAvailability, TorCommand, TorEvent, TorState};

use crate::config::TorRuntimeConfig;
use crate::control::{TorBootstrapPhase, TorControlClient, TorControlConfig};
use crate::logs::LogTail;
use crate::process::TorProcess;
use crate::runtime_events::{RuntimeEventSender, RUNTIME_EVENT_QUEUE_CAPACITY};
use crate::runtime_state::{TorRuntimeSnapshot, TorRuntimeSnapshotReducer};

const CONTROL_BOOTSTRAP_POLL_INTERVAL: Duration = Duration::from_millis(250);
const AUX_TASK_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);

pub struct TorManager {
    command_tx: mpsc::Sender<TorCommand>,
    config_tx: mpsc::Sender<ConfigCommand>,
    state_rx: watch::Receiver<TorState>,
    runtime_state_rx: watch::Receiver<TorRuntimeSnapshot>,
    config_rx: watch::Receiver<TorRuntimeConfig>,
    event_tx: broadcast::Sender<TorEvent>,
}

struct RuntimeSession {
    process: TorProcess,
    logs: LogTail,
    // ControlPort stays session-owned so control actions follow the lifetime
    // of the running Tor instance rather than becoming manager-global state.
    control: Option<TorControlClient>,
    bootstrap_observer: Option<BootstrapObserver>,
}

struct BootstrapObserver {
    shutdown_tx: Option<watch::Sender<bool>>,
    task: Option<JoinHandle<()>>,
}

enum ConfigCommand {
    Set {
        config: Box<TorRuntimeConfig>,
        reply: oneshot::Sender<Result<(), String>>,
    },
}

impl BootstrapObserver {
    fn spawn(config: TorControlConfig, event_tx: RuntimeEventSender) -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let task = tokio::spawn(run_bootstrap_observer(config, event_tx, shutdown_rx));

        Self {
            shutdown_tx: Some(shutdown_tx),
            task: Some(task),
        }
    }

    fn begin_stop(&mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(true);
        }
    }

    async fn finish_stop(&mut self) {
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

    async fn stop(&mut self) {
        self.begin_stop();
        self.finish_stop().await;
    }
}

impl TorManager {
    pub async fn new(config: TorRuntimeConfig) -> Result<Self> {
        validate_runtime_config(&config)?;

        let (command_tx, command_rx) = mpsc::channel(32);
        let (config_tx, config_rx) = mpsc::channel(8);
        let (runtime_event_tx, runtime_event_rx) =
            RuntimeEventSender::channel(RUNTIME_EVENT_QUEUE_CAPACITY);
        let (config_state_tx, config_state_rx) = watch::channel(config.clone());
        let initial_runtime_state = TorRuntimeSnapshot::new(config.control.is_some());
        let (state_tx, state_rx) = watch::channel(initial_runtime_state.tor());
        let (runtime_state_tx, runtime_state_rx) = watch::channel(initial_runtime_state);
        let (event_tx, _) = broadcast::channel(256);

        tokio::spawn(run_state_store(
            runtime_event_rx,
            state_tx,
            runtime_state_tx,
            initial_runtime_state,
            event_tx.clone(),
        ));
        tokio::spawn(run_supervisor(
            config,
            command_rx,
            config_rx,
            config_state_tx,
            runtime_event_tx,
        ));

        Ok(Self {
            command_tx,
            config_tx,
            state_rx,
            runtime_state_rx,
            config_rx: config_state_rx,
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

    pub fn runtime_state_receiver(&self) -> watch::Receiver<TorRuntimeSnapshot> {
        self.runtime_state_rx.clone()
    }

    pub fn runtime_state(&self) -> watch::Receiver<TorRuntimeSnapshot> {
        self.runtime_state_rx.clone()
    }

    pub fn current_runtime_state(&self) -> TorRuntimeSnapshot {
        *self.runtime_state_rx.borrow()
    }

    pub fn current_config(&self) -> TorRuntimeConfig {
        self.config_rx.borrow().clone()
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

    pub async fn set_runtime_config(&self, config: TorRuntimeConfig) -> Result<()> {
        validate_runtime_config(&config)?;

        let (reply_tx, reply_rx) = oneshot::channel();
        self.config_tx
            .send(ConfigCommand::Set {
                config: Box::new(config),
                reply: reply_tx,
            })
            .await
            .map_err(|_| anyhow!("tor runtime supervisor is not available"))?;

        reply_rx
            .await
            .map_err(|_| anyhow!("tor runtime supervisor did not respond to config update"))?
            .map_err(anyhow::Error::msg)
    }
}

async fn run_supervisor(
    mut config: TorRuntimeConfig,
    mut command_rx: mpsc::Receiver<TorCommand>,
    mut config_rx: mpsc::Receiver<ConfigCommand>,
    config_state_tx: watch::Sender<TorRuntimeConfig>,
    runtime_event_tx: RuntimeEventSender,
) {
    let mut session: Option<RuntimeSession> = None;

    loop {
        if session.is_some() {
            let next_action = {
                let active_session = session.as_mut().expect("runtime session exists");
                tokio::select! {
                    command = command_rx.recv() => SupervisorAction::Command(command),
                    config = config_rx.recv() => SupervisorAction::Config(config),
                    wait_result = active_session.process.wait() => SupervisorAction::ProcessExited(wait_result),
                }
            };

            match next_action {
                SupervisorAction::Command(Some(command)) => {
                    handle_command(command, &config, &mut session, &runtime_event_tx).await;
                }
                SupervisorAction::Config(Some(command)) => {
                    handle_config_command(command, &mut config, &session, &config_state_tx).await;
                }
                SupervisorAction::Config(None) => {}
                SupervisorAction::Command(None) => {
                    if let Some(mut active_session) = session.take() {
                        let _ = stop_session(&mut active_session, &config, &runtime_event_tx, true)
                            .await;
                    }
                    break;
                }
                SupervisorAction::ProcessExited(wait_result) => {
                    if let Some(mut active_session) = session.take() {
                        if let Some(observer) = active_session.bootstrap_observer.as_mut() {
                            observer.begin_stop();
                        }
                        active_session.logs.begin_stop();

                        if let Some(observer) = active_session.bootstrap_observer.as_mut() {
                            observer.finish_stop().await;
                        }
                        active_session.logs.finish_stop().await;
                    }

                    publish_exit_event(wait_result, &runtime_event_tx).await;
                }
            }
        } else {
            tokio::select! {
                command = command_rx.recv() => {
                    let Some(command) = command else {
                        break;
                    };

                    handle_command(command, &config, &mut session, &runtime_event_tx).await;
                }
                config_command = config_rx.recv() => {
                    if let Some(command) = config_command {
                        handle_config_command(
                            command,
                            &mut config,
                            &session,
                            &config_state_tx,
                        )
                        .await;
                    }
                }
            };
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
        Ok(_) => {
            emit_event(
                runtime_event_tx,
                TorEvent::ControlAvailabilityChanged(ControlAvailability::Available),
            )
            .await;
            emit_event(runtime_event_tx, TorEvent::IdentityRenewed).await;
        }
        Err(error) => {
            emit_event(
                runtime_event_tx,
                TorEvent::ControlAvailabilityChanged(ControlAvailability::Unavailable),
            )
            .await;
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

async fn handle_config_command(
    command: ConfigCommand,
    config: &mut TorRuntimeConfig,
    session: &Option<RuntimeSession>,
    config_state_tx: &watch::Sender<TorRuntimeConfig>,
) {
    match command {
        ConfigCommand::Set {
            config: next_config,
            reply,
        } => {
            let result = if session.is_some() {
                Err("runtime config cannot be changed while Tor is Starting or Running".to_string())
            } else {
                validate_runtime_config(&next_config)
                    .map_err(|error| error.to_string())
                    .map(|_| {
                        let next_config = *next_config;
                        *config = next_config.clone();
                        let _ = config_state_tx.send(next_config);
                    })
            };

            let _ = reply.send(result);
        }
    }
}

async fn start_session(
    config: &TorRuntimeConfig,
    runtime_event_tx: &RuntimeEventSender,
) -> Result<RuntimeSession> {
    let control = config.control.clone().map(TorControlClient::new);
    let bootstrap_observer = config
        .control
        .clone()
        .map(|control| BootstrapObserver::spawn(control, runtime_event_tx.clone()));
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
            bootstrap_observer,
        }),
        Err(error) => {
            if let Some(mut observer) = bootstrap_observer {
                observer.stop().await;
            }
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
    if let Some(observer) = session.bootstrap_observer.as_mut() {
        observer.begin_stop();
    }

    session.logs.begin_stop();
    let stop_result = session.process.stop(config.stop_timeout).await;

    if let Some(observer) = session.bootstrap_observer.as_mut() {
        observer.finish_stop().await;
    }
    session.logs.finish_stop().await;

    stop_result?;

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

async fn emit_event(event_tx: &RuntimeEventSender, event: TorEvent) {
    event_tx.send(event).await;
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

fn bootstrap_phase_event(last_progress: Option<u8>, phase: &TorBootstrapPhase) -> Option<TorEvent> {
    (last_progress.is_none_or(|previous| phase.progress() > previous))
        .then_some(TorEvent::Bootstrap(phase.progress()))
}

enum SupervisorAction {
    Command(Option<TorCommand>),
    Config(Option<ConfigCommand>),
    ProcessExited(std::io::Result<std::process::ExitStatus>),
}

fn validate_runtime_config(config: &TorRuntimeConfig) -> Result<()> {
    if config.tor_path.as_os_str().is_empty() {
        return Err(anyhow!("runtime config requires a non-empty tor_path"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::broadcast;
    use tokio::time::{sleep, timeout};
    use torq_core::{ControlAvailability, TorEvent};

    use super::TorManager;
    use crate::config::LogMode;
    use crate::config::TorRuntimeConfig;
    use crate::control::{TorBootstrapPhase, TorControlAuth, TorControlConfig};

    #[tokio::test]
    async fn runtime_state_defaults_to_unconfigured_control_without_config() {
        let manager = TorManager::new(TorRuntimeConfig::new(
            "tor.exe",
            unique_test_path("idle-runtime-state.log"),
        ))
        .await
        .unwrap();

        let runtime_state = manager.current_runtime_state();
        assert_eq!(
            runtime_state.control().port(),
            ControlAvailability::Unconfigured
        );
        assert_eq!(
            runtime_state.control().bootstrap_observation(),
            ControlAvailability::Unconfigured
        );
        assert!(!runtime_state.control_configured());
        assert!(!runtime_state.new_identity_available());
        assert!(!runtime_state.uses_control_bootstrap_observation());
    }

    #[tokio::test]
    async fn runtime_config_can_be_updated_while_stopped() {
        let initial_log_path = unique_test_path("config-stopped-initial.log");
        let manager = TorManager::new(
            TorRuntimeConfig::new("tor.exe", &initial_log_path)
                .with_stop_timeout(Duration::from_secs(5)),
        )
        .await
        .unwrap();

        let next_log_path = unique_test_path("config-stopped-next.log");
        let next_config = TorRuntimeConfig::new("tor.exe", &next_log_path)
            .with_torrc_path("C:/Tor/torrc")
            .with_use_torrc(true)
            .with_stop_timeout(Duration::from_secs(2))
            .with_log_poll_interval(Duration::from_millis(100));

        manager
            .set_runtime_config(next_config.clone())
            .await
            .unwrap();

        let current = manager.current_config();
        assert_eq!(current.tor_path, next_config.tor_path);
        assert_eq!(current.log_path, next_config.log_path);
        assert_eq!(current.log_mode, next_config.log_mode);
        assert_eq!(current.torrc_path, next_config.torrc_path);
        assert_eq!(current.use_torrc, next_config.use_torrc);
        assert_eq!(current.args, next_config.args);
        assert_eq!(current.working_dir, next_config.working_dir);
        assert_eq!(current.control, next_config.control);
        assert_eq!(current.stop_timeout, next_config.stop_timeout);
        assert_eq!(current.log_poll_interval, next_config.log_poll_interval);
    }

    #[tokio::test]
    async fn runtime_config_rejects_empty_tor_path() {
        let manager = TorManager::new(TorRuntimeConfig::new(
            "tor.exe",
            unique_test_path("config-validation.log"),
        ))
        .await
        .unwrap();

        let invalid_config =
            TorRuntimeConfig::new("", unique_test_path("config-validation-next.log"));

        let error = manager
            .set_runtime_config(invalid_config)
            .await
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("runtime config requires a non-empty tor_path"));
    }

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
            .with_stop_timeout(Duration::from_secs(5));
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
        assert_eq!(
            manager.current_runtime_state().control().port(),
            ControlAvailability::Unconfigured
        );
        assert!(!manager.current_runtime_state().new_identity_available());

        manager.stop().await.unwrap();
        let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Stopped).await;
        cleanup(&log_path).await;
    }

    #[tokio::test]
    async fn runtime_config_update_is_rejected_while_running() {
        let script = mock_tor_script_path();
        let log_path = unique_test_path("config-running.log");
        let config = TorRuntimeConfig::new("cmd.exe", &log_path)
            .with_args(["/C", script.to_string_lossy().as_ref()])
            .with_stop_timeout(Duration::from_secs(5));
        let manager = TorManager::new(config).await.unwrap();
        let mut events = manager.subscribe_events();

        manager.start().await.unwrap();
        let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Started).await;
        let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Bootstrap(100)).await;

        assert_eq!(
            manager.current_runtime_state().tor().status(),
            torq_core::RuntimeStatus::Running
        );

        let next_config =
            TorRuntimeConfig::new("cmd.exe", unique_test_path("config-while-running.log"));
        let error = manager.set_runtime_config(next_config).await.unwrap_err();

        assert!(error
            .to_string()
            .contains("runtime config cannot be changed while Tor is Starting or Running"));
        assert_eq!(manager.current_config().log_path, log_path);

        manager.stop().await.unwrap();
        let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Stopped).await;
        cleanup(&log_path).await;
    }

    #[tokio::test]
    async fn new_identity_success_emits_identity_renewed() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut tasks = Vec::new();

            for _ in 0..2 {
                let (socket, _) = listener.accept().await.unwrap();
                tasks.push(tokio::spawn(async move {
                    let mut reader = tokio::io::BufReader::new(socket);
                    let mut line = String::new();

                    reader.read_line(&mut line).await.unwrap();
                    assert_eq!(line.trim_end_matches(['\r', '\n']), "AUTHENTICATE");
                    reader.get_mut().write_all(b"250 OK\r\n").await.unwrap();

                    line.clear();
                    reader.read_line(&mut line).await.unwrap();
                    match line.trim_end_matches(['\r', '\n']) {
                        "GETINFO status/bootstrap-phase" => {
                            reader
                                .get_mut()
                                .write_all(
                                    b"250-status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=100 TAG=done SUMMARY=\"Done\"\r\n250 OK\r\n",
                                )
                                .await
                                .unwrap();
                        }
                        "SIGNAL NEWNYM" => {
                            reader.get_mut().write_all(b"250 OK\r\n").await.unwrap();
                        }
                        other => panic!("unexpected control command: {other}"),
                    }
                }));
            }

            for task in tasks {
                task.await.unwrap();
            }
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
            .with_stop_timeout(Duration::from_secs(5));
        let manager = TorManager::new(config).await.unwrap();
        let mut events = manager.subscribe_events();

        manager.start().await.unwrap();
        let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Started).await;

        manager.new_identity().await.unwrap();

        assert_eq!(
            recv_matching_event(&mut events, |event| *event == TorEvent::IdentityRenewed).await,
            TorEvent::IdentityRenewed
        );
        assert_eq!(
            manager.current_runtime_state().control().port(),
            ControlAvailability::Available
        );
        assert!(manager.current_runtime_state().new_identity_available());

        manager.stop().await.unwrap();
        let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Stopped).await;
        server.await.unwrap();
        cleanup(&log_path).await;
    }

    #[test]
    fn bootstrap_phase_event_only_emits_forward_progress() {
        assert_eq!(
            super::bootstrap_phase_event(
                None,
                &TorBootstrapPhase {
                    progress: 15,
                    tag: Some("conn".to_string()),
                    summary: None,
                }
            ),
            Some(TorEvent::Bootstrap(15))
        );
        assert_eq!(
            super::bootstrap_phase_event(
                Some(15),
                &TorBootstrapPhase {
                    progress: 15,
                    tag: Some("conn".to_string()),
                    summary: None,
                }
            ),
            None
        );
        assert_eq!(
            super::bootstrap_phase_event(
                Some(40),
                &TorBootstrapPhase {
                    progress: 20,
                    tag: Some("conn".to_string()),
                    summary: None,
                }
            ),
            None
        );
        assert_eq!(
            super::bootstrap_phase_event(
                Some(40),
                &TorBootstrapPhase {
                    progress: 41,
                    tag: Some("conn".to_string()),
                    summary: None,
                }
            ),
            Some(TorEvent::Bootstrap(41))
        );
    }

    #[tokio::test]
    async fn control_bootstrap_observer_emits_bootstrap_event() {
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
            assert_eq!(
                line.trim_end_matches(['\r', '\n']),
                "GETINFO status/bootstrap-phase"
            );
            reader
                .get_mut()
                .write_all(
                    b"250-status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=25 TAG=starting SUMMARY=\"Starting\"\r\n250 OK\r\n",
                )
                .await
                .unwrap();

            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }

                if line.trim_end_matches(['\r', '\n']) == "GETINFO status/bootstrap-phase"
                    && reader
                        .get_mut()
                        .write_all(
                            b"250-status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=25 TAG=starting SUMMARY=\"Starting\"\r\n250 OK\r\n",
                        )
                        .await
                        .is_err()
                {
                    break;
                }
            }
        });

        let log_path = unique_test_path("control-bootstrap-observer.log");
        let control = TorControlConfig::new(
            address.ip().to_string(),
            address.port(),
            TorControlAuth::Null,
        );
        let config = TorRuntimeConfig::new("powershell.exe", &log_path)
            .with_args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"])
            .with_log_mode(LogMode::External)
            .with_control(control)
            .with_stop_timeout(Duration::from_secs(5));
        let manager = TorManager::new(config).await.unwrap();
        let mut events = manager.subscribe_events();

        manager.start().await.unwrap();
        let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Started).await;

        assert_eq!(
            recv_matching_event(&mut events, |event| *event == TorEvent::Bootstrap(25)).await,
            TorEvent::Bootstrap(25)
        );
        assert_eq!(
            manager.current_runtime_state().control().port(),
            ControlAvailability::Available
        );
        assert_eq!(
            manager
                .current_runtime_state()
                .control()
                .bootstrap_observation(),
            ControlAvailability::Available
        );
        assert!(manager
            .current_runtime_state()
            .uses_control_bootstrap_observation());

        manager.stop().await.unwrap();
        let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Stopped).await;
        server.await.unwrap();
        cleanup(&log_path).await;
    }

    #[tokio::test]
    async fn bootstrap_observer_failure_marks_control_unavailable() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);

        let log_path = unique_test_path("control-bootstrap-unavailable.log");
        let control = TorControlConfig::new(
            address.ip().to_string(),
            address.port(),
            TorControlAuth::Null,
        );
        let config = TorRuntimeConfig::new("powershell.exe", &log_path)
            .with_args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"])
            .with_log_mode(LogMode::External)
            .with_control(control)
            .with_stop_timeout(Duration::from_secs(5));
        let manager = TorManager::new(config).await.unwrap();
        let mut events = manager.subscribe_events();

        manager.start().await.unwrap();
        let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Started).await;
        let _ = recv_matching_event(&mut events, |event| {
            matches!(
                event,
                TorEvent::Warning(message)
                    if message.starts_with("bootstrap observation via ControlPort is unavailable:")
            )
        })
        .await;

        assert_eq!(
            manager.current_runtime_state().control().port(),
            ControlAvailability::Unavailable
        );
        assert_eq!(
            manager
                .current_runtime_state()
                .control()
                .bootstrap_observation(),
            ControlAvailability::Unavailable
        );
        assert!(!manager.current_runtime_state().new_identity_available());
        assert!(!manager
            .current_runtime_state()
            .uses_control_bootstrap_observation());

        manager.stop().await.unwrap();
        let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Stopped).await;
        cleanup(&log_path).await;
    }

    #[tokio::test]
    async fn stop_completes_even_when_control_observer_is_stuck() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            sleep(Duration::from_secs(30)).await;
        });

        let log_path = unique_test_path("control-bootstrap-stuck.log");
        let control = TorControlConfig::new(
            address.ip().to_string(),
            address.port(),
            TorControlAuth::Null,
        );
        let config = TorRuntimeConfig::new("powershell.exe", &log_path)
            .with_args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"])
            .with_log_mode(LogMode::External)
            .with_control(control)
            .with_stop_timeout(Duration::from_millis(750));
        let manager = TorManager::new(config).await.unwrap();
        let mut events = manager.subscribe_events();

        manager.start().await.unwrap();
        let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Started).await;

        timeout(Duration::from_secs(5), manager.stop())
            .await
            .expect("stop should complete even if the control observer is hung")
            .unwrap();
        let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Stopped).await;

        server.abort();
        cleanup(&log_path).await;
    }

    #[tokio::test]
    async fn new_identity_failure_marks_control_unavailable() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (observer_socket, _) = listener.accept().await.unwrap();
            let mut observer = tokio::io::BufReader::new(observer_socket);
            let mut line = String::new();

            observer.read_line(&mut line).await.unwrap();
            assert_eq!(line.trim_end_matches(['\r', '\n']), "AUTHENTICATE");
            observer.get_mut().write_all(b"250 OK\r\n").await.unwrap();

            line.clear();
            observer.read_line(&mut line).await.unwrap();
            assert_eq!(
                line.trim_end_matches(['\r', '\n']),
                "GETINFO status/bootstrap-phase"
            );
            observer
                .get_mut()
                .write_all(
                    b"250-status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=25 TAG=starting SUMMARY=\"Starting\"\r\n250 OK\r\n",
                )
                .await
                .unwrap();

            let (action_socket, _) = listener.accept().await.unwrap();
            let mut action = tokio::io::BufReader::new(action_socket);

            line.clear();
            action.read_line(&mut line).await.unwrap();
            assert_eq!(line.trim_end_matches(['\r', '\n']), "AUTHENTICATE");
            action.get_mut().write_all(b"250 OK\r\n").await.unwrap();

            line.clear();
            action.read_line(&mut line).await.unwrap();
            assert_eq!(line.trim_end_matches(['\r', '\n']), "SIGNAL NEWNYM");
            action
                .get_mut()
                .write_all(b"552 Unrecognized signal\r\n")
                .await
                .unwrap();
        });

        let log_path = unique_test_path("newnym-control-unavailable.log");
        let control = TorControlConfig::new(
            address.ip().to_string(),
            address.port(),
            TorControlAuth::Null,
        );
        let config = TorRuntimeConfig::new("powershell.exe", &log_path)
            .with_args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"])
            .with_log_mode(LogMode::External)
            .with_control(control)
            .with_stop_timeout(Duration::from_secs(5));
        let manager = TorManager::new(config).await.unwrap();
        let mut events = manager.subscribe_events();

        manager.start().await.unwrap();
        let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Started).await;
        let _ = recv_matching_event(&mut events, |event| *event == TorEvent::Bootstrap(25)).await;
        assert_eq!(
            manager.current_runtime_state().control().port(),
            ControlAvailability::Available
        );

        manager.new_identity().await.unwrap();
        let _ = recv_matching_event(&mut events, |event| {
            matches!(
                event,
                TorEvent::Error(message)
                    if message.starts_with("failed to request new identity via ControlPort:")
            )
        })
        .await;

        assert_eq!(
            manager.current_runtime_state().control().port(),
            ControlAvailability::Unavailable
        );
        assert!(!manager.current_runtime_state().new_identity_available());

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
        timeout(Duration::from_secs(15), async {
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
