use serde::{Serialize, Serializer};

/// Error type returned by every Tauri command. Internally everything is `anyhow`;
/// at the IPC boundary the full context chain becomes a single readable message.
#[derive(Debug)]
pub struct CmdError(pub String);

impl<E: Into<anyhow::Error>> From<E> for CmdError {
    fn from(error: E) -> Self {
        CmdError(format!("{:#}", error.into()))
    }
}

impl Serialize for CmdError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

pub type CmdResult<T> = Result<T, CmdError>;

pub fn fail<T>(message: impl Into<String>) -> CmdResult<T> {
    Err(CmdError(message.into()))
}
