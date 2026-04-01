use std::process::ExitStatus;
use std::process::Stdio;

use anyhow::{Context, Result};
use tokio::process::{Child, Command};
use tokio::time::timeout;

use crate::config::TorRuntimeConfig;

pub async fn spawn_process(config: &TorRuntimeConfig) -> Result<Child> {
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

    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

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

pub async fn stop_process(child: &mut Child, stop_timeout: std::time::Duration) -> Result<()> {
    terminate_child(child, stop_timeout).await
}

#[cfg(windows)]
async fn terminate_child(child: &mut Child, stop_timeout: std::time::Duration) -> Result<()> {
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
async fn terminate_child(child: &mut Child, stop_timeout: std::time::Duration) -> Result<()> {
    child
        .start_kill()
        .context("failed to signal tor process for shutdown")?;

    timeout(stop_timeout, child.wait())
        .await
        .context("timed out while waiting for tor to exit")?
        .context("failed while waiting for tor to exit")?;

    Ok(())
}
