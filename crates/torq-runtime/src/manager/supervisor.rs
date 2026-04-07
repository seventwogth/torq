use anyhow::Result;
use tokio::sync::{mpsc, oneshot, watch};
use torq_core::{ControlAvailability, TorCommand, TorEvent};

use crate::config::TorRuntimeConfig;
use crate::control::TorControlClient;
use crate::logs::LogTail;
use crate::process::TorProcess;
use crate::runtime_events::RuntimeEventSender;

use super::bootstrap::BootstrapObserver;
use super::state_store::emit_event;
use super::validate_runtime_config;

struct RuntimeSession {
    process: TorProcess,
    logs: LogTail,
    // ControlPort stays session-owned so control actions follow the lifetime
    // of the running Tor instance rather than becoming manager-global state.
    control: Option<TorControlClient>,
    bootstrap_observer: Option<BootstrapObserver>,
}

pub(super) enum ConfigCommand {
    Set {
        config: Box<TorRuntimeConfig>,
        reply: oneshot::Sender<Result<(), String>>,
    },
}

pub(super) async fn run_supervisor(
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

enum SupervisorAction {
    Command(Option<TorCommand>),
    Config(Option<ConfigCommand>),
    ProcessExited(std::io::Result<std::process::ExitStatus>),
}
