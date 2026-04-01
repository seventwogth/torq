mod error;

pub use error::{TorError, TorResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TorCommand {
    Start,
    Stop,
    Restart,
    NewIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TorEvent {
    LogLine(String),
    Bootstrap(u8),
    Started,
    Stopped,
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TorState {
    pub is_running: bool,
    pub bootstrap: u8,
}

impl TorState {
    pub fn set_bootstrap(&mut self, bootstrap: u8) -> TorResult<()> {
        if bootstrap > 100 {
            return Err(TorError::InvalidBootstrap(bootstrap));
        }

        self.bootstrap = bootstrap;
        Ok(())
    }
}
