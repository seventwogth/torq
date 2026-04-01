use std::path::Path;
use std::process::Stdio;

use anyhow::{anyhow, Context, Result};
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};
use torq_core::{TorCommand, TorEvent, TorState};

use crate::config::TorRuntimeConfig;
use crate::log_parser::classify_log_line;

pub struct TorManager {
    command_tx: mpsc::Sender<TorCommand>,
    state_rx: watch::Receiver<TorState>,
    event_tx: broadcast::Sender<TorEvent>,
}

impl TorManager {
    pub async fn new(config: TorRuntimeConfig) -> Result<Self> {
        let (command_tx, command_rx) = mpsc::channel(32);
        let (state_tx, state_rx) = watch::channel(TorState::default());
        let (event_tx, _) = broadcast::channel(256);

        prepare_log_file(&config.log_path).await?;

        tokio::spawn(run_supervisor(
            config,
            command_rx,
            state_tx,
            event_tx.clone(),
        ));

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

struct ManagedProcess {
    child: Child,
    log_task: JoinHandle<()>,
}

async fn run_supervisor(
    config: TorRuntimeConfig,
    mut command_rx: mpsc::Receiver<TorCommand>,
    state_tx: watch::Sender<TorState>,
    event_tx: broadcast::Sender<TorEvent>,
) {
    let mut process: Option<ManagedProcess> = None;

    loop {
        if process.is_some() {
            tokio::select! {
                command = command_rx.recv() => {
                    let Some(command) = command else {
                        if let Some(active_process) = process.take() {
                            let _ = stop_process(&config, active_process, &state_tx, &event_tx).await;
                        }
                        break;
                    };

                    handle_command(command, &config, &mut process, &state_tx, &event_tx).await;
                }
                wait_result = wait_for_process(process.as_mut().expect("process exists")) => {
                    let message = match wait_result {
                        Ok(status) if status.success() => None,
                        Ok(status) => Some(format!("tor exited unexpectedly with status: {status}")),
                        Err(error) => Some(format!("failed to wait for tor process: {error}")),
                    };

                    if let Some(message) = message {
                        publish_event(&event_tx, TorEvent::Error(message));
                    }

                    if let Some(active_process) = process.take() {
                        active_process.log_task.abort();
                        let _ = active_process.log_task.await;
                    }

                    publish_stopped(&state_tx, &event_tx);
                }
            }
        } else {
            let Some(command) = command_rx.recv().await else {
                break;
            };

            handle_command(command, &config, &mut process, &state_tx, &event_tx).await;
        }
    }
}

async fn handle_command(
    command: TorCommand,
    config: &TorRuntimeConfig,
    process: &mut Option<ManagedProcess>,
    state_tx: &watch::Sender<TorState>,
    event_tx: &broadcast::Sender<TorEvent>,
) {
    match command {
        TorCommand::Start => {
            if process.is_some() {
                publish_event(
                    event_tx,
                    TorEvent::Error("tor is already running".to_string()),
                );
                return;
            }

            match start_process(config, state_tx.clone(), event_tx.clone()).await {
                Ok(started_process) => {
                    *process = Some(started_process);
                    let _ = state_tx.send(TorState {
                        is_running: true,
                        bootstrap: 0,
                    });
                    publish_event(event_tx, TorEvent::Started);
                }
                Err(error) => publish_event(event_tx, TorEvent::Error(error.to_string())),
            }
        }
        TorCommand::Stop => {
            let Some(active_process) = process.take() else {
                publish_event(event_tx, TorEvent::Error("tor is not running".to_string()));
                return;
            };

            if let Err(error) = stop_process(config, active_process, state_tx, event_tx).await {
                publish_event(event_tx, TorEvent::Error(error.to_string()));
            }
        }
        TorCommand::Restart => {
            if let Some(active_process) = process.take() {
                if let Err(error) = stop_process(config, active_process, state_tx, event_tx).await {
                    publish_event(event_tx, TorEvent::Error(error.to_string()));
                }
            }

            match start_process(config, state_tx.clone(), event_tx.clone()).await {
                Ok(started_process) => {
                    *process = Some(started_process);
                    let _ = state_tx.send(TorState {
                        is_running: true,
                        bootstrap: 0,
                    });
                    publish_event(event_tx, TorEvent::Started);
                }
                Err(error) => publish_event(event_tx, TorEvent::Error(error.to_string())),
            }
        }
        TorCommand::NewIdentity => publish_event(
            event_tx,
            TorEvent::LogLine(
                "new identity requested; ControlPort support is not implemented yet".to_string(),
            ),
        ),
    }
}

async fn start_process(
    config: &TorRuntimeConfig,
    state_tx: watch::Sender<TorState>,
    event_tx: broadcast::Sender<TorEvent>,
) -> Result<ManagedProcess> {
    prepare_log_file(&config.log_path).await?;

    let mut command = Command::new(&config.tor_path);
    command.args(&config.args);

    if config.append_log_argument {
        command
            .arg("--Log")
            .arg(format!("notice file {}", config.log_path.display()));
    }

    if let Some(working_dir) = &config.working_dir {
        command.current_dir(working_dir);
    }

    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let child = command.spawn().with_context(|| {
        format!(
            "failed to spawn tor process at {}",
            config.tor_path.display()
        )
    })?;

    let log_path = config.log_path.clone();
    let poll_interval = config.log_poll_interval;
    let log_events = event_tx.clone();

    let log_task = tokio::spawn(async move {
        if let Err(error) =
            tail_log_file(log_path, log_events.clone(), state_tx, poll_interval).await
        {
            let _ = log_events.send(TorEvent::Error(error.to_string()));
        }
    });

    Ok(ManagedProcess { child, log_task })
}

async fn stop_process(
    config: &TorRuntimeConfig,
    mut process: ManagedProcess,
    state_tx: &watch::Sender<TorState>,
    event_tx: &broadcast::Sender<TorEvent>,
) -> Result<()> {
    terminate_child(&mut process.child, config.stop_timeout).await?;
    process.log_task.abort();
    let _ = process.log_task.await;
    publish_stopped(state_tx, event_tx);
    Ok(())
}

async fn prepare_log_file(log_path: &Path) -> Result<()> {
    if let Some(parent) = log_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create log directory {}", parent.display()))?;
    }

    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_path)
        .await
        .with_context(|| format!("failed to prepare log file {}", log_path.display()))?;

    Ok(())
}

async fn wait_for_process(
    process: &mut ManagedProcess,
) -> std::io::Result<std::process::ExitStatus> {
    process.child.wait().await
}

async fn tail_log_file(
    log_path: std::path::PathBuf,
    event_tx: broadcast::Sender<TorEvent>,
    state_tx: watch::Sender<TorState>,
    poll_interval: std::time::Duration,
) -> Result<()> {
    let mut offset = 0_u64;
    let mut pending = Vec::new();

    loop {
        let metadata = match fs::metadata(&log_path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                sleep(poll_interval).await;
                continue;
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to stat log file {}", log_path.display()));
            }
        };

        if metadata.len() < offset {
            offset = 0;
            pending.clear();
        }

        if metadata.len() > offset {
            let mut file = File::open(&log_path)
                .await
                .with_context(|| format!("failed to open log file {}", log_path.display()))?;

            file.seek(std::io::SeekFrom::Start(offset))
                .await
                .with_context(|| format!("failed to seek log file {}", log_path.display()))?;

            let mut chunk = vec![0_u8; (metadata.len() - offset) as usize];
            file.read_exact(&mut chunk)
                .await
                .with_context(|| format!("failed to read log file {}", log_path.display()))?;

            offset = metadata.len();
            pending.extend_from_slice(&chunk);
            drain_pending_lines(&mut pending, &state_tx, &event_tx);
        }

        sleep(poll_interval).await;
    }
}

fn drain_pending_lines(
    pending: &mut Vec<u8>,
    state_tx: &watch::Sender<TorState>,
    event_tx: &broadcast::Sender<TorEvent>,
) {
    while let Some(newline_index) = pending.iter().position(|byte| *byte == b'\n') {
        let line_bytes: Vec<u8> = pending.drain(..=newline_index).collect();
        let line = String::from_utf8_lossy(&line_bytes);
        let line = line.trim_matches(['\r', '\n']).trim().to_string();

        if !line.is_empty() {
            emit_line_events(line, state_tx, event_tx);
        }
    }
}

fn emit_line_events(
    line: String,
    state_tx: &watch::Sender<TorState>,
    event_tx: &broadcast::Sender<TorEvent>,
) {
    for event in classify_log_line(line) {
        match event {
            TorEvent::LogLine(line) => {
                let _ = event_tx.send(TorEvent::LogLine(line));
            }
            TorEvent::Bootstrap(percent) => {
                let _ = state_tx.send_if_modified(|state| {
                    let changed = state.bootstrap != percent || !state.is_running;
                    state.bootstrap = percent;
                    state.is_running = true;
                    changed
                });
                let _ = event_tx.send(TorEvent::Bootstrap(percent));
            }
            other => {
                let _ = event_tx.send(other);
            }
        }
    }
}

fn publish_event(event_tx: &broadcast::Sender<TorEvent>, event: TorEvent) {
    let _ = event_tx.send(event);
}

fn publish_stopped(state_tx: &watch::Sender<TorState>, event_tx: &broadcast::Sender<TorEvent>) {
    let _ = state_tx.send(TorState::default());
    publish_event(event_tx, TorEvent::Stopped);
}

#[cfg(windows)]
async fn terminate_child(child: &mut Child, stop_timeout: std::time::Duration) -> Result<()> {
    let Some(pid) = child.id() else {
        return Ok(());
    };

    let _ = Command::new("taskkill")
        .arg("/PID")
        .arg(pid.to_string())
        .arg("/T")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;

    match timeout(stop_timeout, child.wait()).await {
        Ok(wait_result) => {
            wait_result.context("failed while waiting for tor to stop")?;
            Ok(())
        }
        Err(_) => {
            Command::new("taskkill")
                .arg("/F")
                .arg("/PID")
                .arg(pid.to_string())
                .arg("/T")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
                .context("failed to force-stop tor process")?;

            timeout(stop_timeout, child.wait())
                .await
                .context("timed out while waiting for tor to exit after a forced kill")?
                .context("failed while waiting for tor to exit after a forced kill")?;

            Ok(())
        }
    }
}

#[cfg(not(windows))]
async fn terminate_child(child: &mut Child, stop_timeout: std::time::Duration) -> Result<()> {
    child
        .start_kill()
        .context("failed to signal tor process for shutdown")?;

    timeout(stop_timeout, child.wait())
        .await
        .context("timed out while waiting for tor to exit")?
        .context("failed while waiting for tor to exit")?;

    Ok(())
}
