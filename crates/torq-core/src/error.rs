use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TorError {
    #[error("tor is already running")]
    AlreadyRunning,
    #[error("tor is not running")]
    NotRunning,
    #[error("invalid bootstrap progress: {0}")]
    InvalidBootstrap(u8),
    #[error("command channel is closed")]
    ChannelClosed,
}

pub type TorResult<T> = Result<T, TorError>;
