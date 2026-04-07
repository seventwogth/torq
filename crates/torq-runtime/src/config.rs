use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use thiserror::Error;

use crate::control::{TorControlAuth, TorControlConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogMode {
    // Runtime owns the log destination: it wires tor to this path and may
    // create/truncate the file before starting the process.
    Managed,
    // Runtime only observes a caller-owned log file. In this mode it must not
    // create, truncate, or otherwise prepare the file on the caller's behalf.
    External,
}

impl LogMode {
    pub const fn is_managed(self) -> bool {
        matches!(self, Self::Managed)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Managed => "managed",
            Self::External => "external",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TorRuntimeConfig {
    pub tor_path: PathBuf,
    pub log_path: PathBuf,
    pub log_mode: LogMode,
    pub torrc_path: Option<PathBuf>,
    pub use_torrc: bool,
    pub args: Vec<OsString>,
    pub working_dir: Option<PathBuf>,
    pub control: Option<TorControlConfig>,
    pub stop_timeout: Duration,
    pub log_poll_interval: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RuntimeConfigValidationError {
    #[error("{field} must not be empty")]
    EmptyField { field: &'static str },
}

impl RuntimeConfigValidationError {
    pub const fn empty_field(field: &'static str) -> Self {
        Self::EmptyField { field }
    }
}

impl TorRuntimeConfig {
    pub fn new(tor_path: impl Into<PathBuf>, log_path: impl Into<PathBuf>) -> Self {
        Self {
            tor_path: tor_path.into(),
            log_path: log_path.into(),
            log_mode: LogMode::Managed,
            torrc_path: None,
            use_torrc: false,
            args: Vec::new(),
            working_dir: None,
            control: None,
            stop_timeout: Duration::from_secs(5),
            log_poll_interval: Duration::from_millis(250),
        }
    }

    pub fn from_tor_path(tor_path: impl Into<PathBuf>) -> Self {
        let tor_path = tor_path.into();
        let log_path = tor_path.with_extension("log");
        Self::new(tor_path, log_path)
    }

    pub fn with_args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_log_mode(mut self, log_mode: LogMode) -> Self {
        self.log_mode = log_mode;
        self
    }

    pub fn with_torrc_path(mut self, torrc_path: impl Into<PathBuf>) -> Self {
        self.torrc_path = Some(torrc_path.into());
        self
    }

    pub fn with_use_torrc(mut self, use_torrc: bool) -> Self {
        self.use_torrc = use_torrc;
        self
    }

    pub fn with_working_dir(mut self, working_dir: impl Into<PathBuf>) -> Self {
        self.working_dir = Some(working_dir.into());
        self
    }

    pub fn with_control(mut self, control: TorControlConfig) -> Self {
        self.control = Some(control);
        self
    }

    pub fn with_append_log_argument(mut self, append_log_argument: bool) -> Self {
        self.log_mode = if append_log_argument {
            LogMode::Managed
        } else {
            LogMode::External
        };
        self
    }

    pub fn with_stop_timeout(mut self, stop_timeout: Duration) -> Self {
        self.stop_timeout = stop_timeout;
        self
    }

    pub fn with_log_poll_interval(mut self, log_poll_interval: Duration) -> Self {
        self.log_poll_interval = log_poll_interval;
        self
    }

    pub fn validate(&self) -> Result<(), RuntimeConfigValidationError> {
        validate_runtime_config(self)
    }
}

pub fn validate_runtime_config(
    config: &TorRuntimeConfig,
) -> Result<(), RuntimeConfigValidationError> {
    validate_non_empty_path(&config.tor_path, "tor_path")?;

    if let Some(working_dir) = config.working_dir.as_ref() {
        validate_non_empty_path(working_dir, "working_dir")?;
    }

    if config.use_torrc {
        match config.torrc_path.as_ref() {
            Some(torrc_path) => validate_non_empty_path(torrc_path, "torrc_path")?,
            None => return Err(RuntimeConfigValidationError::empty_field("torrc_path")),
        }
    }

    if let Some(control) = config.control.as_ref() {
        validate_non_empty_string(&control.host, "control.host")?;

        if let TorControlAuth::Cookie { cookie_path } = &control.auth {
            validate_non_empty_path(cookie_path, "control.auth.cookie_path")?;
        }
    }

    Ok(())
}

fn validate_non_empty_string(
    value: &str,
    field: &'static str,
) -> Result<(), RuntimeConfigValidationError> {
    if value.trim().is_empty() {
        return Err(RuntimeConfigValidationError::empty_field(field));
    }

    Ok(())
}

fn validate_non_empty_path(
    value: &Path,
    field: &'static str,
) -> Result<(), RuntimeConfigValidationError> {
    if value.as_os_str().to_string_lossy().trim().is_empty() {
        return Err(RuntimeConfigValidationError::empty_field(field));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::control::{TorControlAuth, TorControlConfig};

    use super::{RuntimeConfigValidationError, TorRuntimeConfig};

    #[test]
    fn rejects_empty_working_dir() {
        let error = TorRuntimeConfig::new("tor.exe", "tor.log")
            .with_working_dir("   ")
            .validate()
            .unwrap_err();

        assert_eq!(
            error,
            RuntimeConfigValidationError::EmptyField {
                field: "working_dir",
            }
        );
    }

    #[test]
    fn rejects_missing_torrc_path_when_enabled() {
        let error = TorRuntimeConfig::new("tor.exe", "tor.log")
            .with_use_torrc(true)
            .validate()
            .unwrap_err();

        assert_eq!(
            error,
            RuntimeConfigValidationError::EmptyField {
                field: "torrc_path",
            }
        );
    }

    #[test]
    fn rejects_empty_torrc_path_when_enabled() {
        let error = TorRuntimeConfig::new("tor.exe", "tor.log")
            .with_torrc_path(" ")
            .with_use_torrc(true)
            .validate()
            .unwrap_err();

        assert_eq!(
            error,
            RuntimeConfigValidationError::EmptyField {
                field: "torrc_path",
            }
        );
    }

    #[test]
    fn rejects_empty_control_host() {
        let error = TorRuntimeConfig::new("tor.exe", "tor.log")
            .with_control(TorControlConfig::new(" ", 9051, TorControlAuth::Null))
            .validate()
            .unwrap_err();

        assert_eq!(
            error,
            RuntimeConfigValidationError::EmptyField {
                field: "control.host",
            }
        );
    }

    #[test]
    fn rejects_empty_cookie_path() {
        let error = TorRuntimeConfig::new("tor.exe", "tor.log")
            .with_control(TorControlConfig::new(
                "127.0.0.1",
                9051,
                TorControlAuth::Cookie {
                    cookie_path: PathBuf::from(" "),
                },
            ))
            .validate()
            .unwrap_err();

        assert_eq!(
            error,
            RuntimeConfigValidationError::EmptyField {
                field: "control.auth.cookie_path",
            }
        );
    }
}
