//! One error type for the core, with a stable code for the UI.

/// Everything the core can refuse or fail with.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// SQLite failed.
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    /// No download with this id.
    #[error("not found: {0}")]
    NotFound(String),
    /// The URL cannot be downloaded (not http/https, not parseable).
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    /// The action does not fit the download's state (e.g. cancel a completed one).
    #[error("invalid state: {0}")]
    InvalidState(String),
    /// A settings value is out of range or unusable.
    #[error("settings: {0}")]
    Settings(String),
    /// The engine refused or failed.
    #[error(transparent)]
    Engine(#[from] mdm_engine::EngineError),
    /// File-system error outside the engine (deleting a part file…).
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    /// A stored JSON column could not be read.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// Result alias for the core.
pub type Result<T> = std::result::Result<T, CoreError>;

impl CoreError {
    /// Stable identifier for UI mapping; engine errors keep the engine's code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Db(_) => "DB",
            Self::NotFound(_) => "NOT_FOUND",
            Self::InvalidUrl(_) => "INVALID_URL",
            Self::InvalidState(_) => "INVALID_STATE",
            Self::Settings(_) => "SETTINGS",
            Self::Engine(e) => e.code(),
            Self::Io(_) => "IO",
            Self::Json(_) => "JSON",
        }
    }
}
