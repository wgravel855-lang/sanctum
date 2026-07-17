//! Error type shared across sanctum-core.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("password hashing error: {0}")]
    Password(String),

    #[error("hosts file: {0}")]
    Hosts(String),

    #[error("blocklist: {0}")]
    Blocklist(String),

    /// A change was refused because a locked ("Cold Turkey") session forbids it.
    #[error("locked session: {0}")]
    Locked(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn other(msg: impl Into<String>) -> Self {
        Error::Other(msg.into())
    }
}
