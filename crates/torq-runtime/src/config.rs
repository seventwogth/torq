use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct TorRuntimeConfig {
    pub tor_path: PathBuf,
    pub log_path: PathBuf,
    pub args: Vec<OsString>,
    pub working_dir: Option<PathBuf>,
    pub append_log_argument: bool,
    pub stop_timeout: Duration,
    pub log_poll_interval: Duration,
}

impl TorRuntimeConfig {
    pub fn new(tor_path: impl Into<PathBuf>, log_path: impl Into<PathBuf>) -> Self {
        Self {
            tor_path: tor_path.into(),
            log_path: log_path.into(),
            args: Vec::new(),
            working_dir: None,
            append_log_argument: true,
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

    pub fn with_working_dir(mut self, working_dir: impl Into<PathBuf>) -> Self {
        self.working_dir = Some(working_dir.into());
        self
    }

    pub fn with_append_log_argument(mut self, append_log_argument: bool) -> Self {
        self.append_log_argument = append_log_argument;
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
}
