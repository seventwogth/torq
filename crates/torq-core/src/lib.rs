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
    status: RuntimeStatus,
    bootstrap: u8,
}

impl Default for TorState {
    fn default() -> Self {
        Self::stopped()
    }
}

impl TorState {
    /// `TorState` mirrors the runtime reducer semantics instead of allowing any
    /// `(status, bootstrap)` pair to exist publicly.
    ///
    /// Invariants:
    /// - `bootstrap` is always in `0..=100`
    /// - `Stopped` and `Failed` always reset bootstrap to `0`
    /// - `Running` always means bootstrap completed (`100`)
    /// - in-progress bootstrap is represented as `Starting(0..=99)`
    pub const fn stopped() -> Self {
        Self {
            status: RuntimeStatus::Stopped,
            bootstrap: 0,
        }
    }

    pub fn starting(bootstrap: u8) -> TorResult<Self> {
        Self::try_from_parts(RuntimeStatus::Starting, bootstrap)
    }

    pub const fn running() -> Self {
        Self {
            status: RuntimeStatus::Running,
            bootstrap: 100,
        }
    }

    pub const fn failed() -> Self {
        Self {
            status: RuntimeStatus::Failed,
            bootstrap: 0,
        }
    }

    pub const fn status(&self) -> RuntimeStatus {
        self.status
    }

    pub const fn bootstrap(&self) -> u8 {
        self.bootstrap
    }

    pub const fn is_running(&self) -> bool {
        matches!(self.status, RuntimeStatus::Running)
    }

    pub const fn is_active(&self) -> bool {
        matches!(
            self.status,
            RuntimeStatus::Starting | RuntimeStatus::Running
        )
    }

    fn try_from_parts(status: RuntimeStatus, bootstrap: u8) -> TorResult<Self> {
        if bootstrap > 100 {
            return Err(TorError::InvalidBootstrap(bootstrap));
        }

        if matches!(status, RuntimeStatus::Stopped | RuntimeStatus::Failed) && bootstrap != 0 {
            return Err(TorError::InvalidRuntimeStatusTransition);
        }

        if matches!(status, RuntimeStatus::Running) && bootstrap != 100 {
            return Err(TorError::InvalidRuntimeStatusTransition);
        }

        if matches!(status, RuntimeStatus::Starting) && bootstrap >= 100 {
            return Err(TorError::InvalidRuntimeStatusTransition);
        }

        Ok(Self { status, bootstrap })
    }
}

#[cfg(test)]
mod tests {
    use super::{RuntimeStatus, TorError, TorEvent, TorState};

    #[test]
    fn stopped_state_resets_bootstrap() {
        let state = TorState::stopped();

        assert_eq!(state.status(), RuntimeStatus::Stopped);
        assert_eq!(state.bootstrap(), 0);
    }

    #[test]
    fn starting_state_accepts_in_progress_bootstrap() {
        let state = TorState::starting(42).unwrap();

        assert_eq!(state.status(), RuntimeStatus::Starting);
        assert_eq!(state.bootstrap(), 42);
        assert!(matches!(TorEvent::Bootstrap(42), TorEvent::Bootstrap(42)));
    }

    #[test]
    fn starting_state_rejects_complete_bootstrap() {
        assert_eq!(
            TorState::starting(100).unwrap_err(),
            TorError::InvalidRuntimeStatusTransition
        );
    }

    #[test]
    fn starting_state_rejects_out_of_range_bootstrap() {
        assert_eq!(
            TorState::starting(101).unwrap_err(),
            TorError::InvalidBootstrap(101)
        );
    }

    #[test]
    fn running_state_implies_completed_bootstrap() {
        let state = TorState::running();

        assert_eq!(state.status(), RuntimeStatus::Running);
        assert_eq!(state.bootstrap(), 100);
        assert!(state.is_running());
        assert!(state.is_active());
    }

    #[test]
    fn failed_state_resets_bootstrap() {
        let state = TorState::failed();

        assert_eq!(state.status(), RuntimeStatus::Failed);
        assert_eq!(state.bootstrap(), 0);
        assert!(!state.is_active());
    }
}
