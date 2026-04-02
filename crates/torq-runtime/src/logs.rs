use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::fs::{self, File};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::task::JoinHandle;
use tokio::time::sleep;
use torq_core::TorEvent;

use crate::runtime_events::RuntimeEventSender;

pub(crate) struct LogTail {
    task: Option<JoinHandle<()>>,
}

impl LogTail {
    pub(crate) async fn spawn(
        log_path: PathBuf,
        poll_interval: Duration,
        event_tx: RuntimeEventSender,
    ) -> Result<Self> {
        let task = tokio::spawn(async move {
            if let Err(error) = tail_log_file(log_path, poll_interval, event_tx.clone()).await {
                event_tx.send(TorEvent::Error(error.to_string())).await;
            }
        });

        Ok(Self { task: Some(task) })
    }

    pub(crate) async fn stop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
            let _ = task.await;
        }
    }
}

pub fn parse_bootstrap_percentage(line: &str) -> Option<u8> {
    let marker = "Bootstrapped ";
    let start = line.find(marker)? + marker.len();
    let remainder = &line[start..];
    let percent_end = remainder.find('%')?;
    let digits = remainder[..percent_end].trim();
    let value = digits.parse::<u8>().ok()?;

    (value <= 100).then_some(value)
}

pub fn classify_log_line(line: impl Into<String>) -> Vec<TorEvent> {
    let line = line.into();
    let mut events = vec![TorEvent::LogLine(line.clone())];
    let lowercase = line.to_ascii_lowercase();

    if let Some(percent) = parse_bootstrap_percentage(&line) {
        events.push(TorEvent::Bootstrap(percent));
    }

    if lowercase.contains("[warn]") {
        events.push(TorEvent::Warning(line));
    }

    events
}

async fn tail_log_file(
    log_path: PathBuf,
    poll_interval: Duration,
    event_tx: RuntimeEventSender,
) -> Result<()> {
    let mut offset = 0_u64;
    let mut pending = Vec::new();

    loop {
        let metadata = match fs::metadata(&log_path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // The tailer is intentionally read-only. In external mode the log
                // file belongs to someone else, so we wait for it instead of
                // creating or repairing it as if torq owned the path.
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
            drain_pending_lines(&mut pending, &event_tx).await;
        }

        sleep(poll_interval).await;
    }
}

async fn drain_pending_lines(pending: &mut Vec<u8>, event_tx: &RuntimeEventSender) {
    while let Some(newline_index) = pending.iter().position(|byte| *byte == b'\n') {
        let line_bytes: Vec<u8> = pending.drain(..=newline_index).collect();
        let line = String::from_utf8_lossy(&line_bytes);
        let line = line.trim_matches(['\r', '\n']).trim();

        if !line.is_empty() {
            event_tx.send_all(classify_log_line(line.to_owned())).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::{classify_log_line, parse_bootstrap_percentage};
    use tokio::fs;
    use tokio::time::sleep;
    use torq_core::TorEvent;

    use crate::runtime_events::RuntimeEventSender;

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

    #[tokio::test]
    async fn spawn_does_not_create_missing_log_file() {
        let missing_path = unique_test_path("external-tail");
        let _ = fs::remove_file(&missing_path).await;

        let (event_tx, _event_rx) = RuntimeEventSender::channel(4);
        let mut tail =
            super::LogTail::spawn(missing_path.clone(), Duration::from_millis(5), event_tx)
                .await
                .unwrap();

        sleep(Duration::from_millis(20)).await;
        assert!(fs::metadata(&missing_path).await.is_err());

        tail.stop().await;
    }

    fn unique_test_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("torq-{label}-{nonce}.log"))
    }
}
