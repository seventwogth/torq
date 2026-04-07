use anyhow::{Context, Result};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use torq_core::{TorCommand, TorEvent, TorState};

use crate::config::{validate_runtime_config, TorRuntimeConfig};
use crate::runtime_events::{RuntimeEventSender, RUNTIME_EVENT_QUEUE_CAPACITY};
use crate::runtime_state::TorRuntimeSnapshot;

mod bootstrap;
mod state_store;
mod supervisor;

#[cfg(test)]
mod tests;

use state_store::run_state_store;
use supervisor::{run_supervisor, ConfigCommand};

pub struct TorManager {
    command_tx: mpsc::Sender<TorCommand>,
    config_tx: mpsc::Sender<ConfigCommand>,
    state_rx: watch::Receiver<TorState>,
    runtime_state_rx: watch::Receiver<TorRuntimeSnapshot>,
    config_rx: watch::Receiver<TorRuntimeConfig>,
    event_tx: broadcast::Sender<TorEvent>,
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

    pub fn state(&self) -> watch::Receiver<TorState> {
        self.state_rx.clone()
    }

    pub fn current_state(&self) -> TorState {
        *self.state_rx.borrow()
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
            .context("tor runtime supervisor is not available")
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
            .context("tor runtime supervisor is not available")?;

        reply_rx
            .await
            .context("tor runtime supervisor did not respond to config update")?
            .map_err(anyhow::Error::msg)
    }
}
