use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TorError {
    #[error("invalid bootstrap progress: {0}")]
    InvalidBootstrap(u8),
    #[error("invalid runtime status transition")]
    InvalidRuntimeStatusTransition,
}

pub type TorResult<T> = Result<T, TorError>;
