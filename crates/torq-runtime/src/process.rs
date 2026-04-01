use std::process::ExitStatus;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::process::{Child, Command};
use tokio::time::timeout;

use crate::config::TorRuntimeConfig;

pub struct TorProcess {
    child: Child,
}

impl TorProcess {
    pub async fn spawn(config: &TorRuntimeConfig) -> Result<Self> {
        prepare_log_file(&config.log_path).await?;
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
    prepare_log_file(&config.log_path).await?;

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

async fn prepare_log_file(log_path: &std::path::Path) -> Result<()> {
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

    if config.append_log_argument {
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
