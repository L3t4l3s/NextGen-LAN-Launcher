use std::path::PathBuf;

/// Crate-wide error type. Variants carry enough context to build a user-facing
/// [`crate::problem::Problem`] without string parsing.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("catalog error: {0}")]
    Catalog(String),
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("invalid launcher.ini: {0}")]
    LauncherIni(String),
    #[error("invalid manifest {path}: {message}")]
    Manifest { path: PathBuf, message: String },
    #[error("archive error: {0}")]
    Archive(String),
    #[error("transport error: {0}")]
    Transport(String),
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("settings error: {0}")]
    Settings(String),
    #[error("launch error: {0}")]
    Launch(String),
    /// A stable, user-facing code (`err.<name>`) the frontend translates.
    #[error("{0}")]
    Code(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io {
            path: path.into(),
            source,
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Settings(e.to_string())
    }
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Error::Http(e.to_string())
    }
}
