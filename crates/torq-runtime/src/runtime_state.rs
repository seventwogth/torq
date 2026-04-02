use torq_core::{ControlAvailability, RuntimeStatus, TorEvent, TorResult, TorState};

/// Operational runtime snapshot for future UI/backend consumers.
///
/// `TorState` stays focused on lifecycle/bootstrap invariants. This snapshot
/// adds only the narrow ControlPort availability slice needed to answer whether
/// control-backed observation and actions are currently usable.
///
/// It intentionally does not model a full control-session lifecycle yet. The
/// detail level here is just enough to distinguish unconfigured, configured but
/// unavailable, and operationally available control-backed paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TorRuntimeSnapshot {
    tor: TorState,
    control: TorControlRuntime,
}

impl TorRuntimeSnapshot {
    pub fn new(control_configured: bool) -> Self {
        Self {
            tor: TorState::default(),
            control: TorControlRuntime::new(control_configured),
        }
    }

    pub const fn tor(&self) -> TorState {
        self.tor
    }

    pub const fn control(&self) -> TorControlRuntime {
        self.control
    }

    pub const fn control_configured(&self) -> bool {
        self.control.port().is_configured()
    }

    pub const fn control_available(&self) -> bool {
        self.control.port().is_available()
    }

    pub const fn bootstrap_observation_available(&self) -> bool {
        self.control.bootstrap_observation().is_available()
    }

    pub const fn new_identity_available(&self) -> bool {
        self.tor.is_active() && self.control_available()
    }

    pub const fn uses_control_bootstrap_observation(&self) -> bool {
        matches!(self.tor.status(), RuntimeStatus::Starting)
            && self.bootstrap_observation_available()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TorControlRuntime {
    port: ControlAvailability,
    bootstrap_observation: ControlAvailability,
}

impl TorControlRuntime {
    pub fn new(configured: bool) -> Self {
        let availability = if configured {
            ControlAvailability::Unavailable
        } else {
            ControlAvailability::Unconfigured
        };

        Self {
            port: availability,
            bootstrap_observation: availability,
        }
    }

    pub const fn port(&self) -> ControlAvailability {
        self.port
    }

    pub const fn bootstrap_observation(&self) -> ControlAvailability {
        self.bootstrap_observation
    }

    fn reset_for_session_boundary(&mut self) {
        let reset = if self.port.is_configured() {
            ControlAvailability::Unavailable
        } else {
            ControlAvailability::Unconfigured
        };

        self.port = reset;
        self.bootstrap_observation = reset;
    }
}

pub struct TorRuntimeSnapshotReducer;

impl TorRuntimeSnapshotReducer {
    pub fn apply_event(state: &mut TorRuntimeSnapshot, event: &TorEvent) -> TorResult<()> {
        crate::state::apply_event(&mut state.tor, event)?;

        match event {
            TorEvent::ControlAvailabilityChanged(availability) => {
                state.control.port = *availability;
            }
            TorEvent::BootstrapObservationAvailabilityChanged(availability) => {
                state.control.bootstrap_observation = *availability;
            }
            TorEvent::Stopped | TorEvent::StartFailed(_) | TorEvent::Crashed(_) => {
                state.control.reset_for_session_boundary();
            }
            TorEvent::Started
            | TorEvent::IdentityRenewed
            | TorEvent::LogLine(_)
            | TorEvent::Bootstrap(_)
            | TorEvent::Warning(_)
            | TorEvent::Error(_) => {}
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use torq_core::{ControlAvailability, RuntimeStatus, TorEvent};

    use super::{TorRuntimeSnapshot, TorRuntimeSnapshotReducer};

    #[test]
    fn snapshot_defaults_to_unconfigured_control_without_config() {
        let snapshot = TorRuntimeSnapshot::new(false);

        assert_eq!(snapshot.tor().status(), RuntimeStatus::Stopped);
        assert_eq!(snapshot.control().port(), ControlAvailability::Unconfigured);
        assert_eq!(
            snapshot.control().bootstrap_observation(),
            ControlAvailability::Unconfigured
        );
        assert!(!snapshot.new_identity_available());
        assert!(!snapshot.uses_control_bootstrap_observation());
    }

    #[test]
    fn reducer_keeps_tor_and_control_state_separate() {
        let mut snapshot = TorRuntimeSnapshot::new(true);

        for event in [
            TorEvent::Started,
            TorEvent::ControlAvailabilityChanged(ControlAvailability::Available),
            TorEvent::BootstrapObservationAvailabilityChanged(ControlAvailability::Available),
            TorEvent::Bootstrap(45),
            TorEvent::ControlAvailabilityChanged(ControlAvailability::Unavailable),
        ] {
            TorRuntimeSnapshotReducer::apply_event(&mut snapshot, &event).unwrap();
        }

        assert_eq!(snapshot.tor().status(), RuntimeStatus::Starting);
        assert_eq!(snapshot.tor().bootstrap(), 45);
        assert_eq!(snapshot.control().port(), ControlAvailability::Unavailable);
        assert_eq!(
            snapshot.control().bootstrap_observation(),
            ControlAvailability::Available
        );
        assert!(!snapshot.new_identity_available());
        assert!(snapshot.uses_control_bootstrap_observation());
    }

    #[test]
    fn reducer_resets_configured_control_to_unavailable_when_session_ends() {
        let mut snapshot = TorRuntimeSnapshot::new(true);

        for event in [
            TorEvent::Started,
            TorEvent::ControlAvailabilityChanged(ControlAvailability::Available),
            TorEvent::BootstrapObservationAvailabilityChanged(ControlAvailability::Available),
            TorEvent::Bootstrap(100),
            TorEvent::Stopped,
        ] {
            TorRuntimeSnapshotReducer::apply_event(&mut snapshot, &event).unwrap();
        }

        assert_eq!(snapshot.tor().status(), RuntimeStatus::Stopped);
        assert_eq!(snapshot.control().port(), ControlAvailability::Unavailable);
        assert_eq!(
            snapshot.control().bootstrap_observation(),
            ControlAvailability::Unavailable
        );
    }
}
