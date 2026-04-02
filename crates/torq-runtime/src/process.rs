use std::path::Path;
use std::process::ExitStatus;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::process::{Child, Command};
use tokio::time::timeout;

use crate::config::{LogMode, TorRuntimeConfig};

pub struct TorProcess {
    child: Child,
}

impl TorProcess {
    pub async fn spawn(config: &TorRuntimeConfig) -> Result<Self> {
        prepare_log_destination(config.log_mode, &config.log_path).await?;
        let mut command = build_command(config);
        let child = command.spawn().with_context(|| {
            format!(
                "failed to spawn tor process at {}",
                config.tor_path.display()
            )
        })?;

        Ok(Self { child })
    }

    pub async fn wait(&mut self) -> std::io::Result<ExitStatus> {
        self.child.wait().await
    }

    pub async fn stop(&mut self, stop_timeout: Duration) -> Result<()> {
        terminate_child(&mut self.child, stop_timeout).await
    }
}

pub async fn spawn_process(config: &TorRuntimeConfig) -> Result<Child> {
    prepare_log_destination(config.log_mode, &config.log_path).await?;

    let mut command = build_command(config);
    command.spawn().with_context(|| {
        format!(
            "failed to spawn tor process at {}",
            config.tor_path.display()
        )
    })
}

pub async fn wait_for_exit(child: &mut Child) -> std::io::Result<ExitStatus> {
    child.wait().await
}

pub async fn stop_process(child: &mut Child, stop_timeout: Duration) -> Result<()> {
    terminate_child(child, stop_timeout).await
}

async fn prepare_log_destination(log_mode: LogMode, log_path: &Path) -> Result<()> {
    if !log_mode.is_managed() {
        return Ok(());
    }

    // Managed mode means the runtime owns this log path: it points tor at the
    // file and may create/truncate it before startup. External mode must not
    // touch the caller-owned file at all.
    let parent = log_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty());

    if let Some(parent) = parent {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create log directory {}", parent.display()))?;
    }

    tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_path)
        .await
        .with_context(|| format!("failed to prepare log file {}", log_path.display()))?;

    Ok(())
}

fn build_command(config: &TorRuntimeConfig) -> Command {
    let mut command = Command::new(&config.tor_path);
    command.args(&config.args);

    if config.log_mode.is_managed() {
        command
            .arg("--Log")
            .arg(format!("notice file {}", config.log_path.display()));
    }

    if let Some(working_dir) = &config.working_dir {
        command.current_dir(working_dir);
    }

    command.kill_on_drop(true);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    command
}

#[cfg(windows)]
async fn terminate_child(child: &mut Child, stop_timeout: Duration) -> Result<()> {
    let Some(pid) = child.id() else {
        return Ok(());
    };

    let _ = Command::new("taskkill")
        .arg("/PID")
        .arg(pid.to_string())
        .arg("/T")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;

    match timeout(stop_timeout, child.wait()).await {
        Ok(wait_result) => {
            wait_result.context("failed while waiting for tor to stop")?;
            Ok(())
        }
        Err(_) => {
            Command::new("taskkill")
                .arg("/F")
                .arg("/PID")
                .arg(pid.to_string())
                .arg("/T")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
                .context("failed to force-stop tor process")?;

            timeout(stop_timeout, child.wait())
                .await
                .context("timed out while waiting for tor to exit after a forced kill")?
                .context("failed while waiting for tor to exit after a forced kill")?;

            Ok(())
        }
    }
}

#[cfg(not(windows))]
async fn terminate_child(child: &mut Child, stop_timeout: Duration) -> Result<()> {
    child
        .start_kill()
        .context("failed to signal tor process for shutdown")?;

    timeout(stop_timeout, child.wait())
        .await
        .context("timed out while waiting for tor to exit")?
        .context("failed while waiting for tor to exit")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use tokio::fs;

    use super::{build_command, prepare_log_destination};
    use crate::config::{LogMode, TorRuntimeConfig};

    #[tokio::test]
    async fn managed_mode_creates_and_truncates_log_file() {
        let log_path = unique_test_path("managed-log");
        if let Some(parent) = log_path.parent() {
            let _ = fs::remove_dir_all(parent).await;
        }

        prepare_log_destination(LogMode::Managed, &log_path)
            .await
            .unwrap();
        assert_eq!(fs::read(&log_path).await.unwrap(), Vec::<u8>::new());

        fs::write(&log_path, b"stale log data").await.unwrap();
        prepare_log_destination(LogMode::Managed, &log_path)
            .await
            .unwrap();
        assert_eq!(fs::read(&log_path).await.unwrap(), Vec::<u8>::new());

        cleanup(&log_path).await;
    }

    #[tokio::test]
    async fn external_mode_leaves_log_file_untouched() {
        let log_path = unique_test_path("external-log");
        if let Some(parent) = log_path.parent() {
            fs::create_dir_all(parent).await.unwrap();
        }

        fs::write(&log_path, b"caller owned").await.unwrap();
        prepare_log_destination(LogMode::External, &log_path)
            .await
            .unwrap();
        assert_eq!(fs::read(&log_path).await.unwrap(), b"caller owned");

        let missing_path = unique_test_path("external-missing");
        let _ = fs::remove_file(&missing_path).await;
        prepare_log_destination(LogMode::External, &missing_path)
            .await
            .unwrap();
        assert!(fs::metadata(&missing_path).await.is_err());

        cleanup(&log_path).await;
        cleanup(&missing_path).await;
    }

    #[test]
    fn managed_mode_adds_log_argument_and_external_mode_does_not() {
        let managed = TorRuntimeConfig::new("tor.exe", "managed.log");
        let external =
            TorRuntimeConfig::new("tor.exe", "external.log").with_log_mode(LogMode::External);

        let managed_debug = format!("{:?}", build_command(&managed).as_std());
        let external_debug = format!("{:?}", build_command(&external).as_std());

        assert!(managed_debug.contains("--Log"));
        assert!(managed_debug.contains("notice file managed.log"));
        assert!(!external_debug.contains("--Log"));
        assert!(!external_debug.contains("notice file external.log"));
    }

    async fn cleanup(path: &Path) {
        let _ = fs::remove_file(path).await;
        if let Some(parent) = path.parent() {
            let _ = fs::remove_dir(parent).await;
        }
    }

    fn unique_test_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir()
            .join("torq-process-tests")
            .join(label)
            .join(format!("{nonce}.log"))
    }
}
