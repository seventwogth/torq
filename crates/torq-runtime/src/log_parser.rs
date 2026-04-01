//! Helpers for turning Tor stdout/stderr lines into structured events.

use torq_core::TorEvent;

/// Parse a bootstrap percentage from a Tor log line.
///
/// Returns `Some(percent)` for inputs like `Bootstrapped 45%`, and `None`
/// when the line does not contain a valid bootstrap percentage.
pub fn parse_bootstrap_percentage(line: &str) -> Option<u8> {
    let marker = "Bootstrapped ";
    let start = line.find(marker)? + marker.len();
    let remainder = &line[start..];
    let percent_end = remainder.find('%')?;
    let digits = remainder[..percent_end].trim();
    let value = digits.parse::<u8>().ok()?;

    (value <= 100).then_some(value)
}

/// Convert a raw Tor log line into the structured events the runtime uses.
///
/// The caller typically emits `TorEvent::LogLine` for every line first, then
/// uses the optional bootstrap event to update state and broadcast progress.
pub fn classify_log_line(line: impl Into<String>) -> Vec<TorEvent> {
    let line = line.into();
    let mut events = vec![TorEvent::LogLine(line.clone())];

    if let Some(percent) = parse_bootstrap_percentage(&line) {
        events.push(TorEvent::Bootstrap(percent));
    }

    events
}

#[cfg(test)]
mod tests {
    use super::{classify_log_line, parse_bootstrap_percentage};
    use torq_core::TorEvent;

    #[test]
    fn parses_bootstrap_percentage() {
        assert_eq!(
            parse_bootstrap_percentage("Bootstrapped 45%: Loading"),
            Some(45)
        );
        assert_eq!(
            parse_bootstrap_percentage("Bootstrapped 0%: Starting"),
            Some(0)
        );
        assert_eq!(
            parse_bootstrap_percentage("Bootstrapped 100%: Done"),
            Some(100)
        );
    }

    #[test]
    fn rejects_invalid_bootstrap_lines() {
        assert_eq!(parse_bootstrap_percentage("Bootstrapped 101%"), None);
        assert_eq!(parse_bootstrap_percentage("Bootstrapped foo%"), None);
        assert_eq!(parse_bootstrap_percentage("Tor is ready"), None);
    }

    #[test]
    fn classifies_log_lines_in_order() {
        let events = classify_log_line("Bootstrapped 45%: Loading");

        assert_eq!(
            events,
            vec![
                TorEvent::LogLine("Bootstrapped 45%: Loading".to_string()),
                TorEvent::Bootstrap(45),
            ]
        );
    }

    #[test]
    fn classifies_plain_log_lines() {
        let events = classify_log_line("Bootstrapped halfway there");

        assert_eq!(
            events,
            vec![TorEvent::LogLine("Bootstrapped halfway there".to_string())]
        );
    }
}
