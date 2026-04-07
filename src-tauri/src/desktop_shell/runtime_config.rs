//! Serde-friendly runtime config DTOs and validation for the desktop backend.
//!
//! This module intentionally stays narrow:
//! - it defines the request/response shape for `get_runtime_config` and
//!   `set_runtime_config`
//! - it converts to and from `torq_runtime::TorRuntimeConfig`
//! - it validates only the minimal constraints needed for a safe desktop API

use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use torq_runtime::{
    LogMode, RuntimeConfigValidationError, TorControlAuth, TorControlConfig, TorRuntimeConfig,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeLogMode {
    Managed,
    External,
}

impl From<LogMode> for RuntimeLogMode {
    fn from(value: LogMode) -> Self {
        match value {
            LogMode::Managed => Self::Managed,
            LogMode::External => Self::External,
        }
    }
}

impl From<RuntimeLogMode> for LogMode {
    fn from(value: RuntimeLogMode) -> Self {
        match value {
            RuntimeLogMode::Managed => Self::Managed,
            RuntimeLogMode::External => Self::External,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeControlAuth {
    Null,
    Cookie { cookie_path: String },
}

impl From<&TorControlAuth> for RuntimeControlAuth {
    fn from(value: &TorControlAuth) -> Self {
        match value {
            TorControlAuth::Null => Self::Null,
            TorControlAuth::Cookie { cookie_path } => Self::Cookie {
                cookie_path: path_to_string(cookie_path),
            },
        }
    }
}

impl TryFrom<RuntimeControlAuth> for TorControlAuth {
    type Error = RuntimeConfigError;

    fn try_from(value: RuntimeControlAuth) -> Result<Self, Self::Error> {
        match value {
            RuntimeControlAuth::Null => Ok(TorControlAuth::Null),
            RuntimeControlAuth::Cookie { cookie_path } => Ok(TorControlAuth::Cookie {
                cookie_path: PathBuf::from(cookie_path),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeControlConfig {
    pub host: String,
    pub port: u16,
    pub auth: RuntimeControlAuth,
}

impl From<&TorControlConfig> for RuntimeControlConfig {
    fn from(value: &TorControlConfig) -> Self {
        Self {
            host: value.host.clone(),
            port: value.port,
            auth: RuntimeControlAuth::from(&value.auth),
        }
    }
}

impl TryFrom<RuntimeControlConfig> for TorControlConfig {
    type Error = RuntimeConfigError;

    fn try_from(value: RuntimeControlConfig) -> Result<Self, Self::Error> {
        Ok(TorControlConfig::new(
            value.host,
            value.port,
            value.auth.try_into()?,
        ))
    }
}

/// DTO shared by the runtime config get/set API and persistence layer.
///
/// The same payload shape is used for reads and writes, so desktop commands can
/// stay thin and keep all conversion logic in one place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeConfigDto {
    pub tor_path: String,
    pub log_path: String,
    pub log_mode: RuntimeLogMode,
    #[serde(default)]
    pub torrc_path: Option<String>,
    #[serde(default)]
    pub use_torrc: bool,
    pub args: Vec<String>,
    pub working_dir: Option<String>,
    pub control: Option<RuntimeControlConfig>,
    pub stop_timeout_ms: u64,
    pub log_poll_interval_ms: u64,
}

pub type RuntimeConfigRequest = RuntimeConfigDto;
pub type RuntimeConfigResponse = RuntimeConfigDto;

impl RuntimeConfigDto {
    pub fn from_runtime_config(config: &TorRuntimeConfig) -> Self {
        Self {
            tor_path: path_to_string(&config.tor_path),
            log_path: path_to_string(&config.log_path),
            log_mode: RuntimeLogMode::from(config.log_mode),
            torrc_path: config.torrc_path.as_ref().map(|path| path_to_string(path)),
            use_torrc: config.use_torrc,
            args: config
                .args
                .iter()
                .map(|value| value.to_string_lossy().into_owned())
                .collect(),
            working_dir: config.working_dir.as_ref().map(|path| path_to_string(path)),
            control: config.control.as_ref().map(RuntimeControlConfig::from),
            stop_timeout_ms: millis(config.stop_timeout),
            log_poll_interval_ms: millis(config.log_poll_interval),
        }
    }

    pub fn validate(&self) -> Result<(), RuntimeConfigError> {
        self.clone().into_runtime_config_unvalidated()?.validate()?;
        Ok(())
    }

    pub fn try_into_runtime_config(self) -> Result<TorRuntimeConfig, RuntimeConfigError> {
        let config = self.into_runtime_config_unvalidated()?;
        config.validate()?;
        Ok(config)
    }

    fn into_runtime_config_unvalidated(self) -> Result<TorRuntimeConfig, RuntimeConfigError> {
        let Self {
            tor_path,
            log_path,
            log_mode,
            torrc_path,
            use_torrc,
            args,
            working_dir,
            control,
            stop_timeout_ms,
            log_poll_interval_ms,
        } = self;

        let mut config = TorRuntimeConfig::new(tor_path, log_path)
            .with_log_mode(log_mode.into())
            .with_use_torrc(use_torrc)
            .with_args(args.into_iter().map(OsString::from));

        if let Some(torrc_path) = normalize_optional_string(torrc_path) {
            config = config.with_torrc_path(torrc_path);
        }

        if let Some(working_dir) = working_dir {
            config = config.with_working_dir(working_dir);
        }

        if let Some(control) = control {
            config = config.with_control(control.try_into()?);
        }

        config = config
            .with_stop_timeout(Duration::from_millis(stop_timeout_ms))
            .with_log_poll_interval(Duration::from_millis(log_poll_interval_ms));

        Ok(config)
    }
}

impl From<&TorRuntimeConfig> for RuntimeConfigDto {
    fn from(value: &TorRuntimeConfig) -> Self {
        Self::from_runtime_config(value)
    }
}

impl TryFrom<RuntimeConfigDto> for TorRuntimeConfig {
    type Error = RuntimeConfigError;

    fn try_from(value: RuntimeConfigDto) -> Result<Self, Self::Error> {
        value.try_into_runtime_config()
    }
}

pub fn get_runtime_config_response(config: &TorRuntimeConfig) -> RuntimeConfigResponse {
    RuntimeConfigResponse::from(config)
}

pub fn set_runtime_config_request(
    request: RuntimeConfigRequest,
) -> Result<TorRuntimeConfig, RuntimeConfigError> {
    request.try_into()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeConfigError {
    EmptyField { field: &'static str },
}

impl RuntimeConfigError {
    pub fn empty_field(field: &'static str) -> Self {
        Self::EmptyField { field }
    }
}

impl From<RuntimeConfigValidationError> for RuntimeConfigError {
    fn from(value: RuntimeConfigValidationError) -> Self {
        match value {
            RuntimeConfigValidationError::EmptyField { field } => Self::EmptyField { field },
        }
    }
}

impl fmt::Display for RuntimeConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyField { field } => write!(f, "{field} must not be empty"),
        }
    }
}

impl Error for RuntimeConfigError {}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim().to_string();
        (!value.is_empty()).then_some(value)
    })
}

fn path_to_string(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

fn millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use serde::{Deserialize, Serialize};
    use torq_runtime::{LogMode, TorControlAuth, TorControlConfig, TorRuntimeConfig};

    use super::{
        get_runtime_config_response, set_runtime_config_request, RuntimeConfigDto,
        RuntimeConfigError, RuntimeControlAuth, RuntimeControlConfig, RuntimeLogMode,
    };

    #[test]
    fn dto_round_trips_runtime_config_shape() {
        let runtime_config = TorRuntimeConfig::new("tor.exe", "tor.log")
            .with_log_mode(LogMode::External)
            .with_torrc_path("C:/Tor/torrc")
            .with_use_torrc(true)
            .with_args(["--foo", "--bar"])
            .with_working_dir("C:/Tor")
            .with_control(TorControlConfig::new(
                "127.0.0.1",
                9051,
                TorControlAuth::Null,
            ))
            .with_stop_timeout(Duration::from_secs(7))
            .with_log_poll_interval(Duration::from_millis(125));

        let dto = RuntimeConfigDto::from(&runtime_config);

        assert_eq!(dto.tor_path, "tor.exe");
        assert_eq!(dto.log_path, "tor.log");
        assert_eq!(dto.log_mode, RuntimeLogMode::External);
        assert_eq!(dto.torrc_path.as_deref(), Some("C:/Tor/torrc"));
        assert!(dto.use_torrc);
        assert_eq!(dto.args, vec!["--foo".to_string(), "--bar".to_string()]);
        assert_eq!(dto.working_dir.as_deref(), Some("C:/Tor"));
        assert_eq!(
            dto.control,
            Some(RuntimeControlConfig {
                host: "127.0.0.1".to_string(),
                port: 9051,
                auth: RuntimeControlAuth::Null,
            })
        );
        assert_eq!(dto.stop_timeout_ms, 7000);
        assert_eq!(dto.log_poll_interval_ms, 125);

        let rebuilt = set_runtime_config_request(dto).unwrap();

        assert_eq!(rebuilt.tor_path, PathBuf::from("tor.exe"));
        assert_eq!(rebuilt.log_path, PathBuf::from("tor.log"));
        assert_eq!(rebuilt.log_mode, LogMode::External);
        assert_eq!(rebuilt.torrc_path, Some(PathBuf::from("C:/Tor/torrc")));
        assert!(rebuilt.use_torrc);
        assert_eq!(
            rebuilt.args,
            vec![
                std::ffi::OsString::from("--foo"),
                std::ffi::OsString::from("--bar")
            ]
        );
        assert_eq!(rebuilt.working_dir, Some(PathBuf::from("C:/Tor")));
        assert_eq!(
            rebuilt.control,
            Some(TorControlConfig::new(
                "127.0.0.1",
                9051,
                TorControlAuth::Null
            ))
        );
        assert_eq!(rebuilt.stop_timeout, Duration::from_secs(7));
        assert_eq!(rebuilt.log_poll_interval, Duration::from_millis(125));
    }

    #[test]
    fn rejects_empty_tor_path() {
        let dto = RuntimeConfigDto {
            tor_path: "   ".to_string(),
            log_path: "tor.log".to_string(),
            log_mode: RuntimeLogMode::Managed,
            torrc_path: None,
            use_torrc: false,
            args: vec![],
            working_dir: None,
            control: None,
            stop_timeout_ms: 5_000,
            log_poll_interval_ms: 250,
        };

        let error = dto.try_into_runtime_config().unwrap_err();

        assert_eq!(error, RuntimeConfigError::EmptyField { field: "tor_path" });
    }

    #[test]
    fn rejects_empty_cookie_path() {
        let dto = RuntimeConfigDto {
            tor_path: "tor.exe".to_string(),
            log_path: "tor.log".to_string(),
            log_mode: RuntimeLogMode::Managed,
            torrc_path: None,
            use_torrc: false,
            args: vec![],
            working_dir: None,
            control: Some(RuntimeControlConfig {
                host: "127.0.0.1".to_string(),
                port: 9051,
                auth: RuntimeControlAuth::Cookie {
                    cookie_path: " ".to_string(),
                },
            }),
            stop_timeout_ms: 5_000,
            log_poll_interval_ms: 250,
        };

        let error = dto.try_into_runtime_config().unwrap_err();

        assert_eq!(
            error,
            RuntimeConfigError::EmptyField {
                field: "control.auth.cookie_path",
            }
        );
    }

    #[test]
    fn rejects_empty_control_host() {
        let dto = RuntimeConfigDto {
            tor_path: "tor.exe".to_string(),
            log_path: "tor.log".to_string(),
            log_mode: RuntimeLogMode::Managed,
            torrc_path: None,
            use_torrc: false,
            args: vec![],
            working_dir: None,
            control: Some(RuntimeControlConfig {
                host: " ".to_string(),
                port: 9051,
                auth: RuntimeControlAuth::Null,
            }),
            stop_timeout_ms: 5_000,
            log_poll_interval_ms: 250,
        };

        let error = dto.try_into_runtime_config().unwrap_err();

        assert_eq!(
            error,
            RuntimeConfigError::EmptyField {
                field: "control.host",
            }
        );
    }

    #[test]
    fn rejects_empty_working_dir() {
        let dto = RuntimeConfigDto {
            tor_path: "tor.exe".to_string(),
            log_path: "tor.log".to_string(),
            log_mode: RuntimeLogMode::Managed,
            torrc_path: None,
            use_torrc: false,
            args: vec![],
            working_dir: Some(" ".to_string()),
            control: None,
            stop_timeout_ms: 5_000,
            log_poll_interval_ms: 250,
        };

        let error = dto.try_into_runtime_config().unwrap_err();

        assert_eq!(
            error,
            RuntimeConfigError::EmptyField {
                field: "working_dir",
            }
        );
    }

    #[test]
    fn rejects_missing_torrc_path_when_enabled() {
        let dto = RuntimeConfigDto {
            tor_path: "tor.exe".to_string(),
            log_path: "tor.log".to_string(),
            log_mode: RuntimeLogMode::Managed,
            torrc_path: Some(" ".to_string()),
            use_torrc: true,
            args: vec![],
            working_dir: None,
            control: None,
            stop_timeout_ms: 5_000,
            log_poll_interval_ms: 250,
        };

        let error = dto.try_into_runtime_config().unwrap_err();

        assert_eq!(
            error,
            RuntimeConfigError::EmptyField {
                field: "torrc_path"
            }
        );
    }

    #[test]
    fn get_runtime_config_response_uses_current_runtime_shape() {
        let runtime_config = TorRuntimeConfig::from_tor_path("tor.exe").with_args(["--help"]);

        let response = get_runtime_config_response(&runtime_config);

        assert_eq!(response.tor_path, "tor.exe");
        assert_eq!(response.args, vec!["--help".to_string()]);
    }

    #[test]
    fn dto_is_serde_friendly() {
        fn assert_serde<T>()
        where
            T: Serialize + for<'de> Deserialize<'de>,
        {
        }

        assert_serde::<RuntimeConfigDto>();
        assert_serde::<RuntimeControlConfig>();
        assert_serde::<RuntimeControlAuth>();
        assert_serde::<RuntimeLogMode>();
    }
}
