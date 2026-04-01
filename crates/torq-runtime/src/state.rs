use torq_core::{RuntimeStatus, TorEvent, TorResult, TorState};

pub fn apply_event(state: &mut TorState, event: &TorEvent) -> TorResult<()> {
    match event {
        TorEvent::Started => {
            state.status = RuntimeStatus::Starting;
        }
        TorEvent::Stopped => {
            *state = TorState::stopped();
        }
        TorEvent::StartFailed(_) | TorEvent::Crashed(_) => {
            state.status = RuntimeStatus::Failed;
            state.bootstrap = 0;
        }
        TorEvent::Bootstrap(bootstrap) => {
            state.set_bootstrap(*bootstrap)?;
            state.status = if *bootstrap >= 100 {
                RuntimeStatus::Running
            } else {
                RuntimeStatus::Starting
            };
        }
        TorEvent::Warning(_) | TorEvent::Error(_) | TorEvent::LogLine(_) => {}
    }

    state.validate()?;
    Ok(())
}

pub fn reduce_events<I>(events: I) -> TorResult<TorState>
where
    I: IntoIterator<Item = TorEvent>,
{
    let mut state = TorState::default();

    for event in events {
        apply_event(&mut state, &event)?;
    }

    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::{apply_event, reduce_events};
    use torq_core::{RuntimeStatus, TorEvent, TorState};

    #[test]
    fn started_moves_to_starting() {
        let mut state = TorState::default();

        apply_event(&mut state, &TorEvent::Started).unwrap();

        assert_eq!(state.status, RuntimeStatus::Starting);
        assert_eq!(state.bootstrap, 0);
    }

    #[test]
    fn bootstrap_hundred_marks_running() {
        let mut state = TorState::default();

        apply_event(&mut state, &TorEvent::Bootstrap(100)).unwrap();

        assert_eq!(state.status, RuntimeStatus::Running);
        assert_eq!(state.bootstrap, 100);
    }

    #[test]
    fn stopped_resets_state() {
        let mut state = TorState {
            status: RuntimeStatus::Running,
            bootstrap: 70,
        };

        apply_event(&mut state, &TorEvent::Stopped).unwrap();

        assert_eq!(state, TorState::stopped());
    }

    #[test]
    fn reduce_events_applies_sequence() {
        let state = reduce_events(vec![
            TorEvent::Started,
            TorEvent::Bootstrap(8),
            TorEvent::Bootstrap(100),
        ])
        .unwrap();

        assert_eq!(state.status, RuntimeStatus::Running);
        assert_eq!(state.bootstrap, 100);
    }
}
