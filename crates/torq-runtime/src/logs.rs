use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::sleep;
use torq_core::TorEvent;

pub struct LogTail {
    task: Option<JoinHandle<()>>,
}

impl LogTail {
    pub async fn spawn(
        log_path: PathBuf,
        poll_interval: Duration,
        event_tx: mpsc::UnboundedSender<TorEvent>,
    ) -> Result<Self> {
        prepare_log_file(&log_path).await?;

        let task = tokio::spawn(async move {
            if let Err(error) = tail_log_file(log_path, poll_interval, event_tx.clone()).await {
                let _ = event_tx.send(TorEvent::Error(error.to_string()));
            }
        });

        Ok(Self { task: Some(task) })
    }

    pub async fn stop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
            let _ = task.await;
        }
    }
}

pub async fn prepare_log_file(log_path: &Path) -> Result<()> {
    if let Some(parent) = log_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create log directory {}", parent.display()))?;
    }

    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_path)
        .await
        .with_context(|| format!("failed to prepare log file {}", log_path.display()))?;

    Ok(())
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
    event_tx: mpsc::UnboundedSender<TorEvent>,
) -> Result<()> {
    let mut offset = 0_u64;
    let mut pending = Vec::new();

    loop {
        let metadata = match fs::metadata(&log_path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
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
            drain_pending_lines(&mut pending, &event_tx);
        }

        sleep(poll_interval).await;
    }
}

fn drain_pending_lines(pending: &mut Vec<u8>, event_tx: &mpsc::UnboundedSender<TorEvent>) {
    while let Some(newline_index) = pending.iter().position(|byte| *byte == b'\n') {
        let line_bytes: Vec<u8> = pending.drain(..=newline_index).collect();
        let line = String::from_utf8_lossy(&line_bytes);
        let line = line.trim_matches(['\r', '\n']).trim();

        if !line.is_empty() {
            for event in classify_log_line(line.to_owned()) {
                let _ = event_tx.send(event);
            }
        }
    }
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
}
