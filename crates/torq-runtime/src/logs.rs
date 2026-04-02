use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::fs::{self, File};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::sleep;
use torq_core::TorEvent;

use crate::runtime_events::RuntimeEventSender;

pub(crate) struct LogTail {
    shutdown_tx: Option<watch::Sender<bool>>,
    task: Option<JoinHandle<()>>,
}

impl LogTail {
    pub(crate) async fn spawn(
        log_path: PathBuf,
        poll_interval: Duration,
        event_tx: RuntimeEventSender,
    ) -> Result<Self> {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let task = tokio::spawn(async move {
            if let Err(error) =
                tail_log_file(log_path, poll_interval, event_tx.clone(), shutdown_rx).await
            {
                event_tx.send(TorEvent::Error(error.to_string())).await;
            }
        });

        Ok(Self {
            shutdown_tx: Some(shutdown_tx),
            task: Some(task),
        })
    }

    pub(crate) async fn stop(&mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            // Stop is cooperative so the tailer can do one final pass, drain any
            // buffered bytes, and emit a trailing line that never received '\n'.
            let _ = shutdown_tx.send(true);
        }

        if let Some(task) = self.task.take() {
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
        events.push(TorEvent::Warning(line.clone()));
    }

    // Tor uses bracketed severity markers in log lines. We preserve the raw
    // LogLine event and additionally surface warn/error semantics explicitly.
    if lowercase.contains("[err]") || lowercase.contains("[error]") {
        events.push(TorEvent::Error(line));
    }

    events
}

async fn tail_log_file(
    log_path: PathBuf,
    poll_interval: Duration,
    event_tx: RuntimeEventSender,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<()> {
    let mut offset = 0_u64;
    let mut pending = Vec::new();

    loop {
        process_log_file(&log_path, &mut offset, &mut pending, &event_tx, false).await?;

        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
            _ = sleep(poll_interval) => {}
        }
    }

    process_log_file(&log_path, &mut offset, &mut pending, &event_tx, true).await
}

async fn process_log_file(
    log_path: &Path,
    offset: &mut u64,
    pending: &mut Vec<u8>,
    event_tx: &RuntimeEventSender,
    flush_trailing_line: bool,
) -> Result<()> {
    let metadata = match fs::metadata(log_path).await {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if !flush_trailing_line {
                // The tailer is intentionally read-only. In external mode the
                // log file belongs to someone else, so we wait for it instead
                // of creating or repairing it as if torq owned the path.
            }
            None
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to stat log file {}", log_path.display()));
        }
    };

    if let Some(metadata) = metadata {
        if metadata.len() < *offset {
            *offset = 0;
            pending.clear();
        }

        if metadata.len() > *offset {
            let mut file = File::open(log_path)
                .await
                .with_context(|| format!("failed to open log file {}", log_path.display()))?;

            file.seek(std::io::SeekFrom::Start(*offset))
                .await
                .with_context(|| format!("failed to seek log file {}", log_path.display()))?;

            let mut chunk = vec![0_u8; (metadata.len() - *offset) as usize];
            file.read_exact(&mut chunk)
                .await
                .with_context(|| format!("failed to read log file {}", log_path.display()))?;

            *offset = metadata.len();
            pending.extend_from_slice(&chunk);
        }
    }

    drain_pending_lines(pending, event_tx, flush_trailing_line).await;
    Ok(())
}

async fn drain_pending_lines(
    pending: &mut Vec<u8>,
    event_tx: &RuntimeEventSender,
    flush_trailing_line: bool,
) {
    while let Some(newline_index) = pending.iter().position(|byte| *byte == b'\n') {
        let line_bytes: Vec<u8> = pending.drain(..=newline_index).collect();
        emit_line_bytes(line_bytes, event_tx).await;
    }

    if flush_trailing_line && !pending.is_empty() {
        // A final drain emits the last buffered line even if the producer never
        // wrote a trailing newline, so EOF/shutdown does not silently drop it.
        let trailing_bytes = std::mem::take(pending);
        emit_line_bytes(trailing_bytes, event_tx).await;
    }
}

async fn emit_line_bytes(line_bytes: Vec<u8>, event_tx: &RuntimeEventSender) {
    let line = String::from_utf8_lossy(&line_bytes);
    let line = line.trim_matches(['\r', '\n']).trim();

    if !line.is_empty() {
        event_tx.send_all(classify_log_line(line.to_owned())).await;
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

    #[test]
    fn classifies_warn_and_error_lines_without_losing_logline() {
        let warn_events = classify_log_line("[warn] circuit is slow");
        assert_eq!(
            warn_events,
            vec![
                TorEvent::LogLine("[warn] circuit is slow".to_string()),
                TorEvent::Warning("[warn] circuit is slow".to_string()),
            ]
        );

        let err_events = classify_log_line("Bootstrapped 45%: [err] handshake failed");
        assert_eq!(
            err_events,
            vec![
                TorEvent::LogLine("Bootstrapped 45%: [err] handshake failed".to_string()),
                TorEvent::Bootstrap(45),
                TorEvent::Error("Bootstrapped 45%: [err] handshake failed".to_string()),
            ]
        );

        let error_events = classify_log_line("[error] fatal issue");
        assert_eq!(
            error_events,
            vec![
                TorEvent::LogLine("[error] fatal issue".to_string()),
                TorEvent::Error("[error] fatal issue".to_string()),
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

    #[tokio::test]
    async fn stop_flushes_trailing_line_without_newline_once() {
        let log_path = unique_test_path("tail-stop-flush");
        fs::write(&log_path, b"[err] final message without newline")
            .await
            .unwrap();

        let (event_tx, mut event_rx) = RuntimeEventSender::channel(8);
        let mut tail = super::LogTail::spawn(log_path.clone(), Duration::from_millis(5), event_tx)
            .await
            .unwrap();

        sleep(Duration::from_millis(20)).await;
        assert!(event_rx.try_recv().is_err());

        tail.stop().await;

        assert_eq!(
            event_rx.recv().await,
            Some(TorEvent::LogLine(
                "[err] final message without newline".to_string()
            ))
        );
        assert_eq!(
            event_rx.recv().await,
            Some(TorEvent::Error(
                "[err] final message without newline".to_string()
            ))
        );
        assert!(event_rx.try_recv().is_err());

        let _ = fs::remove_file(&log_path).await;
    }

    fn unique_test_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("torq-{label}-{nonce}.log"))
    }
}
