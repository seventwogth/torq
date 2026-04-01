mod error;

pub use error::{TorError, TorResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RuntimeStatus {
    #[default]
    Stopped,
    Starting,
    Running,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TorCommand {
    Start,
    Stop,
    Restart,
    NewIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TorEvent {
    Started,
    Stopped,
    StartFailed(String),
    Crashed(String),
    LogLine(String),
    Bootstrap(u8),
    Warning(String),
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TorState {
    pub status: RuntimeStatus,
    pub bootstrap: u8,
}

impl Default for TorState {
    fn default() -> Self {
        Self::stopped()
    }
}

impl TorState {
    pub fn new(status: RuntimeStatus, bootstrap: u8) -> TorResult<Self> {
        let state = Self { status, bootstrap };
        state.validate()?;
        Ok(state)
    }

    pub const fn stopped() -> Self {
        Self {
            status: RuntimeStatus::Stopped,
            bootstrap: 0,
        }
    }

    pub const fn is_running(&self) -> bool {
        matches!(
            self.status,
            RuntimeStatus::Starting | RuntimeStatus::Running
        )
    }

    pub fn validate(&self) -> TorResult<()> {
        if self.bootstrap > 100 {
            return Err(TorError::InvalidBootstrap(self.bootstrap));
        }

        if matches!(self.status, RuntimeStatus::Stopped) && self.bootstrap != 0 {
            return Err(TorError::InvalidRuntimeStatusTransition);
        }

        Ok(())
    }

    pub fn set_bootstrap(&mut self, bootstrap: u8) -> TorResult<()> {
        if bootstrap > 100 {
            return Err(TorError::InvalidBootstrap(bootstrap));
        }

        self.bootstrap = bootstrap;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{RuntimeStatus, TorEvent, TorState};

    #[test]
    fn stopped_state_requires_zero_bootstrap() {
        assert!(TorState::new(RuntimeStatus::Stopped, 0).is_ok());
        assert!(TorState::new(RuntimeStatus::Stopped, 1).is_err());
    }

    #[test]
    fn bootstrap_events_can_be_represented_in_state() {
        let mut state = TorState::new(RuntimeStatus::Starting, 0).unwrap();
        state.set_bootstrap(42).unwrap();

        assert_eq!(state.bootstrap, 42);
        assert!(matches!(TorEvent::Bootstrap(42), TorEvent::Bootstrap(42)));
    }
}
