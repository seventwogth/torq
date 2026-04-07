use serde::Serialize;
use torq_core::{ControlAvailability, RuntimeStatus, TorState};
use torq_runtime::{TorControlRuntime, TorRuntimeSnapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStatusView {
    Stopped,
    Starting,
    Running,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlAvailabilityView {
    Unconfigured,
    Unavailable,
    Available,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TorStateView {
    pub status: RuntimeStatusView,
    pub bootstrap: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TorControlRuntimeView {
    pub port: ControlAvailabilityView,
    pub bootstrap_observation: ControlAvailabilityView,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TorRuntimeSnapshotView {
    pub tor: TorStateView,
    pub control: TorControlRuntimeView,
    pub control_configured: bool,
    pub control_available: bool,
    pub bootstrap_observation_available: bool,
    pub new_identity_available: bool,
    pub uses_control_bootstrap_observation: bool,
}

impl From<RuntimeStatus> for RuntimeStatusView {
    fn from(value: RuntimeStatus) -> Self {
        match value {
            RuntimeStatus::Stopped => Self::Stopped,
            RuntimeStatus::Starting => Self::Starting,
            RuntimeStatus::Running => Self::Running,
            RuntimeStatus::Failed => Self::Failed,
        }
    }
}

impl From<ControlAvailability> for ControlAvailabilityView {
    fn from(value: ControlAvailability) -> Self {
        match value {
            ControlAvailability::Unconfigured => Self::Unconfigured,
            ControlAvailability::Unavailable => Self::Unavailable,
            ControlAvailability::Available => Self::Available,
        }
    }
}

impl From<TorState> for TorStateView {
    fn from(value: TorState) -> Self {
        Self {
            status: value.status().into(),
            bootstrap: value.bootstrap(),
        }
    }
}

impl From<TorControlRuntime> for TorControlRuntimeView {
    fn from(value: TorControlRuntime) -> Self {
        Self {
            port: value.port().into(),
            bootstrap_observation: value.bootstrap_observation().into(),
        }
    }
}

impl From<TorRuntimeSnapshot> for TorRuntimeSnapshotView {
    fn from(value: TorRuntimeSnapshot) -> Self {
        Self {
            tor: value.tor().into(),
            control: value.control().into(),
            control_configured: value.control_configured(),
            control_available: value.control_available(),
            bootstrap_observation_available: value.bootstrap_observation_available(),
            new_identity_available: value.new_identity_available(),
            uses_control_bootstrap_observation: value.uses_control_bootstrap_observation(),
        }
    }
}
