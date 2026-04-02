use torq_core::{TorEvent, TorResult, TorState};

pub struct TorStateReducer;

impl TorStateReducer {
    pub fn apply_event(state: &mut TorState, event: &TorEvent) -> TorResult<()> {
        match event {
            TorEvent::Started => {
                *state = TorState::starting(0)?;
            }
            TorEvent::Stopped => {
                *state = TorState::stopped();
            }
            TorEvent::StartFailed(_) | TorEvent::Crashed(_) => {
                *state = TorState::failed();
            }
            TorEvent::Bootstrap(bootstrap) => {
                *state = if *bootstrap == 100 {
                    TorState::running()
                } else {
                    TorState::starting(*bootstrap)?
                };
            }
            TorEvent::Warning(_) | TorEvent::Error(_) | TorEvent::LogLine(_) => {}
        }

        Ok(())
    }

    pub fn reduce_events<I>(events: I) -> TorResult<TorState>
    where
        I: IntoIterator<Item = TorEvent>,
    {
        let mut state = TorState::default();

        for event in events {
            Self::apply_event(&mut state, &event)?;
        }

        Ok(state)
    }
}

pub fn apply_event(state: &mut TorState, event: &TorEvent) -> TorResult<()> {
    TorStateReducer::apply_event(state, event)
}

pub fn reduce_events<I>(events: I) -> TorResult<TorState>
where
    I: IntoIterator<Item = TorEvent>,
{
    TorStateReducer::reduce_events(events)
}

#[cfg(test)]
mod tests {
    use super::{apply_event, reduce_events, TorStateReducer};
    use torq_core::{RuntimeStatus, TorEvent, TorState};

    #[test]
    fn started_moves_to_starting() {
        let mut state = TorState::default();

        apply_event(&mut state, &TorEvent::Started).unwrap();

        assert_eq!(state.status(), RuntimeStatus::Starting);
        assert_eq!(state.bootstrap(), 0);
    }

    #[test]
    fn bootstrap_hundred_marks_running() {
        let mut state = TorState::default();

        TorStateReducer::apply_event(&mut state, &TorEvent::Bootstrap(100)).unwrap();

        assert_eq!(state.status(), RuntimeStatus::Running);
        assert_eq!(state.bootstrap(), 100);
    }

    #[test]
    fn bootstrap_above_hundred_is_rejected() {
        let mut state = TorState::default();

        assert!(TorStateReducer::apply_event(&mut state, &TorEvent::Bootstrap(101)).is_err());
        assert_eq!(state, TorState::stopped());
    }

    #[test]
    fn stopped_resets_state() {
        let mut state = TorState::running();

        apply_event(&mut state, &TorEvent::Stopped).unwrap();

        assert_eq!(state, TorState::stopped());
    }

    #[test]
    fn crash_moves_to_failed_state() {
        let mut state = TorState::starting(42).unwrap();

        apply_event(&mut state, &TorEvent::Crashed("boom".to_string())).unwrap();

        assert_eq!(state, TorState::failed());
    }

    #[test]
    fn reduce_events_applies_sequence() {
        let state = reduce_events(vec![
            TorEvent::Started,
            TorEvent::Bootstrap(8),
            TorEvent::Bootstrap(100),
        ])
        .unwrap();

        assert_eq!(state.status(), RuntimeStatus::Running);
        assert_eq!(state.bootstrap(), 100);
    }
}
