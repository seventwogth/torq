use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use torq_core::{ControlAvailability, TorEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeActivityToneView {
    Neutral,
    Success,
    Warning,
    Danger,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeActivityKindView {
    Lifecycle,
    Bootstrap,
    Control,
    Identity,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TorRuntimeActivityView {
    pub id: u64,
    pub timestamp_ms: u128,
    pub kind: RuntimeActivityKindView,
    pub tone: RuntimeActivityToneView,
    pub title: String,
    pub details: Option<String>,
    pub coalesce_key: Option<String>,
}

impl TorRuntimeActivityView {
    fn new(
        id: u64,
        kind: RuntimeActivityKindView,
        tone: RuntimeActivityToneView,
        title: impl Into<String>,
        details: Option<String>,
        coalesce_key: Option<String>,
    ) -> Self {
        Self {
            id,
            timestamp_ms: current_timestamp_ms(),
            kind,
            tone,
            title: title.into(),
            details,
            coalesce_key,
        }
    }
}

pub fn runtime_activity_from_event(event: &TorEvent, id: u64) -> Option<TorRuntimeActivityView> {
    match event {
        TorEvent::Started => Some(TorRuntimeActivityView::new(
            id,
            RuntimeActivityKindView::Lifecycle,
            RuntimeActivityToneView::Success,
            "Tor started",
            None,
            None::<String>,
        )),
        TorEvent::Stopped => Some(TorRuntimeActivityView::new(
            id,
            RuntimeActivityKindView::Lifecycle,
            RuntimeActivityToneView::Neutral,
            "Tor stopped",
            None,
            None::<String>,
        )),
        TorEvent::IdentityRenewed => Some(TorRuntimeActivityView::new(
            id,
            RuntimeActivityKindView::Identity,
            RuntimeActivityToneView::Success,
            "New identity requested",
            None,
            None::<String>,
        )),
        TorEvent::StartFailed(message) => Some(TorRuntimeActivityView::new(
            id,
            RuntimeActivityKindView::Error,
            RuntimeActivityToneView::Danger,
            "Tor failed to start",
            Some(message.clone()),
            None::<String>,
        )),
        TorEvent::Crashed(message) => Some(TorRuntimeActivityView::new(
            id,
            RuntimeActivityKindView::Error,
            RuntimeActivityToneView::Danger,
            "Tor crashed",
            Some(message.clone()),
            None::<String>,
        )),
        TorEvent::Bootstrap(progress) => Some(TorRuntimeActivityView::new(
            id,
            RuntimeActivityKindView::Bootstrap,
            if *progress == 100 {
                RuntimeActivityToneView::Success
            } else {
                RuntimeActivityToneView::Info
            },
            format!("Bootstrap: {progress}%"),
            None,
            Some("bootstrap".to_string()),
        )),
        TorEvent::ControlAvailabilityChanged(availability) => {
            let (title, tone) = match availability {
                ControlAvailability::Available => (
                    "ControlPort became available",
                    RuntimeActivityToneView::Success,
                ),
                ControlAvailability::Unavailable => (
                    "ControlPort became unavailable",
                    RuntimeActivityToneView::Warning,
                ),
                ControlAvailability::Unconfigured => (
                    "ControlPort is unconfigured",
                    RuntimeActivityToneView::Neutral,
                ),
            };

            Some(TorRuntimeActivityView::new(
                id,
                RuntimeActivityKindView::Control,
                tone,
                title,
                None,
                None::<String>,
            ))
        }
        TorEvent::BootstrapObservationAvailabilityChanged(availability) => {
            let (title, tone) = match availability {
                ControlAvailability::Available => (
                    "Bootstrap observation became available",
                    RuntimeActivityToneView::Success,
                ),
                ControlAvailability::Unavailable => (
                    "Bootstrap observation became unavailable",
                    RuntimeActivityToneView::Warning,
                ),
                ControlAvailability::Unconfigured => (
                    "Bootstrap observation is unconfigured",
                    RuntimeActivityToneView::Neutral,
                ),
            };

            Some(TorRuntimeActivityView::new(
                id,
                RuntimeActivityKindView::Control,
                tone,
                title,
                None,
                None::<String>,
            ))
        }
        TorEvent::Warning(message) => Some(TorRuntimeActivityView::new(
            id,
            RuntimeActivityKindView::Warning,
            RuntimeActivityToneView::Warning,
            "Runtime warning",
            Some(message.clone()),
            None::<String>,
        )),
        TorEvent::Error(message) => Some(TorRuntimeActivityView::new(
            id,
            RuntimeActivityKindView::Error,
            RuntimeActivityToneView::Danger,
            "Runtime error",
            Some(message.clone()),
            None::<String>,
        )),
        TorEvent::LogLine(_) => None,
    }
}

fn current_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis())
}

#[cfg(test)]
mod tests {
    use torq_core::{ControlAvailability, TorEvent};

    use super::{runtime_activity_from_event, RuntimeActivityKindView, RuntimeActivityToneView};

    #[test]
    fn bootstrap_events_coalesce_under_stable_key() {
        let activity = runtime_activity_from_event(&TorEvent::Bootstrap(42), 7).unwrap();

        assert_eq!(activity.id, 7);
        assert_eq!(activity.kind, RuntimeActivityKindView::Bootstrap);
        assert_eq!(activity.tone, RuntimeActivityToneView::Info);
        assert_eq!(activity.coalesce_key.as_deref(), Some("bootstrap"));
    }

    #[test]
    fn control_availability_is_mapped_to_control_activity() {
        let activity = runtime_activity_from_event(
            &TorEvent::ControlAvailabilityChanged(ControlAvailability::Unavailable),
            3,
        )
        .unwrap();

        assert_eq!(activity.kind, RuntimeActivityKindView::Control);
        assert_eq!(activity.tone, RuntimeActivityToneView::Warning);
        assert_eq!(activity.title, "ControlPort became unavailable");
    }
}
