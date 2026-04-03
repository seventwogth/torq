use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use torq_runtime::TorRuntimeConfig;

use crate::runtime_config::{RuntimeConfigDto, RuntimeConfigError};

pub struct RuntimeConfigStore {
    path: PathBuf,
    current: RwLock<TorRuntimeConfig>,
}

impl RuntimeConfigStore {
    pub fn new(path: PathBuf, fallback: TorRuntimeConfig) -> Result<Self, ConfigStoreError> {
        let current = if path.exists() {
            read_config_file(&path)?
        } else {
            fallback
        };

        Ok(Self {
            path,
            current: RwLock::new(current),
        })
    }

    pub fn load(&self) -> Result<TorRuntimeConfig, ConfigStoreError> {
        if !self.path.exists() {
            return self.get();
        }

        let config = read_config_file(&self.path)?;
        self.set(config.clone())?;
        Ok(config)
    }

    pub fn save(&self, config: TorRuntimeConfig) -> Result<(), ConfigStoreError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|source| ConfigStoreError::Io {
                action: "create config directory",
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let payload = RuntimeConfigDto::from(&config);
        let json = serde_json::to_string_pretty(&payload).map_err(|source| {
            ConfigStoreError::Serialize {
                path: self.path.clone(),
                source,
            }
        })?;

        fs::write(&self.path, format!("{json}\n")).map_err(|source| ConfigStoreError::Io {
            action: "write runtime config",
            path: self.path.clone(),
            source,
        })
    }

    pub fn get(&self) -> Result<TorRuntimeConfig, ConfigStoreError> {
        self.current
            .read()
            .map(|config| config.clone())
            .map_err(|_| ConfigStoreError::LockPoisoned)
    }

    pub fn set(&self, config: TorRuntimeConfig) -> Result<(), ConfigStoreError> {
        self.current
            .write()
            .map(|mut current| *current = config)
            .map_err(|_| ConfigStoreError::LockPoisoned)
    }
}

#[derive(Debug)]
pub enum ConfigStoreError {
    Io {
        action: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    Serialize {
        path: PathBuf,
        source: serde_json::Error,
    },
    Invalid(RuntimeConfigError),
    LockPoisoned,
}

impl fmt::Display for ConfigStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                action,
                path,
                source,
            } => {
                write!(f, "failed to {action} at {}: {source}", path.display())
            }
            Self::Parse { path, source } => {
                write!(
                    f,
                    "failed to parse runtime config at {}: {source}",
                    path.display()
                )
            }
            Self::Serialize { path, source } => {
                write!(
                    f,
                    "failed to serialize runtime config for {}: {source}",
                    path.display()
                )
            }
            Self::Invalid(source) => write!(f, "invalid runtime config: {source}"),
            Self::LockPoisoned => write!(f, "runtime config store is unavailable"),
        }
    }
}

impl Error for ConfigStoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            Self::Serialize { source, .. } => Some(source),
            Self::Invalid(source) => Some(source),
            Self::LockPoisoned => None,
        }
    }
}

impl From<RuntimeConfigError> for ConfigStoreError {
    fn from(value: RuntimeConfigError) -> Self {
        Self::Invalid(value)
    }
}

fn read_config_file(path: &Path) -> Result<TorRuntimeConfig, ConfigStoreError> {
    let contents = fs::read_to_string(path).map_err(|source| ConfigStoreError::Io {
        action: "read runtime config",
        path: path.to_path_buf(),
        source,
    })?;
    let dto = serde_json::from_str::<RuntimeConfigDto>(&contents).map_err(|source| {
        ConfigStoreError::Parse {
            path: path.to_path_buf(),
            source,
        }
    })?;

    dto.try_into_runtime_config()
        .map_err(ConfigStoreError::from)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use torq_runtime::{LogMode, TorRuntimeConfig};

    use super::RuntimeConfigStore;

    #[test]
    fn missing_file_uses_fallback_config() {
        let path = unique_test_path("missing");
        let fallback = TorRuntimeConfig::new("tor.exe", "tor.log");

        let store = RuntimeConfigStore::new(path, fallback.clone()).unwrap();

        assert_eq!(store.get().unwrap().tor_path, fallback.tor_path);
        assert_eq!(store.get().unwrap().log_path, fallback.log_path);
    }

    #[test]
    fn save_persists_config_and_load_current_reads_it_back() {
        let path = unique_test_path("roundtrip");
        let fallback = TorRuntimeConfig::new("tor.exe", "tor.log");
        let store = RuntimeConfigStore::new(path.clone(), fallback).unwrap();
        let config = TorRuntimeConfig::new("C:/Tor/tor.exe", "C:/Tor/tor.log")
            .with_log_mode(LogMode::External)
            .with_args(["--ClientOnly", "1"])
            .with_working_dir("C:/Tor")
            .with_stop_timeout(Duration::from_secs(9))
            .with_log_poll_interval(Duration::from_millis(500));

        store.save(config.clone()).unwrap();
        let loaded = store.load().unwrap();

        assert_eq!(loaded.tor_path, config.tor_path);
        assert_eq!(loaded.log_path, config.log_path);
        assert_eq!(loaded.log_mode, config.log_mode);
        assert_eq!(loaded.args, config.args);
        assert_eq!(loaded.working_dir, config.working_dir);
        assert_eq!(loaded.stop_timeout, config.stop_timeout);
        assert_eq!(loaded.log_poll_interval, config.log_poll_interval);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn set_updates_in_memory_value() {
        let path = unique_test_path("set");
        let fallback = TorRuntimeConfig::new("tor.exe", "tor.log");
        let store = RuntimeConfigStore::new(path, fallback).unwrap();
        let next = TorRuntimeConfig::new("updated-tor.exe", "updated.log");

        store.set(next.clone()).unwrap();

        assert_eq!(store.get().unwrap().tor_path, next.tor_path);
        assert_eq!(store.get().unwrap().log_path, next.log_path);
    }

    fn unique_test_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("torq-config-store-{label}-{nonce}.json"))
    }
}
