use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("No config file found. Set $CEO_CONFIG, create ~/.config/ceo/config.toml, or create ./ceo.toml")]
    NotFound,
    #[error("Failed to parse config: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("Failed to serialize config: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("Failed to read config file {path}")]
    ReadFile { path: PathBuf, source: std::io::Error },
    #[error("Failed to write config file {path}")]
    WriteFile { path: PathBuf, source: std::io::Error },
    #[error("Failed to create config directory {path}")]
    CreateDir { path: PathBuf, source: std::io::Error },
    #[error("Unknown config key: {0}")]
    UnknownKey(String),
    #[error("Invalid value for {key}: {message}")]
    InvalidValue { key: String, message: String },
    #[error("No model configured for prompt kind: {0}")]
    NoModel(String),
    #[error("No tools configured for prompt kind: {0}")]
    NoTools(String),
}

#[derive(Debug, thiserror::Error)]
pub enum GhError {
    #[error("Failed to run gh CLI. Is it installed? https://cli.github.com")]
    NotInstalled(#[source] std::io::Error),
    #[error("gh is not authenticated. Run `gh auth login` first.")]
    NotAuthenticated,
    #[error("gh command failed: {0}")]
    CommandFailed(String),
    #[error("Failed to parse GitHub JSON: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("Agent command '{command}' not found. Check your config.")]
    NotFound { command: String, source: std::io::Error },
    #[error("Failed to read agent output")]
    OutputRead(#[source] std::io::Error),
    #[error("Agent exited with error: {0}")]
    ExitError(String),
}

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Database not found at {0}. Run `ceo sync` first.")]
    NotFound(std::path::PathBuf),
    #[error("Failed to create data directory {path}")]
    CreateDir { path: std::path::PathBuf, source: std::io::Error },
    #[error("Failed to delete database at {path}")]
    DeleteFailed { path: std::path::PathBuf, source: std::io::Error },
}

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error(transparent)]
    Gh(#[from] GhError),
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("Git error: {0}")]
    Git(String),
}

#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error(transparent)]
    Gh(#[from] GhError),
    #[error(transparent)]
    Agent(#[from] AgentError),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Db(#[from] DbError),
}
