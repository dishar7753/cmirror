use thiserror::Error;

#[derive(Error, Debug)]
pub enum MirrorError {
    #[error("IO operation failed: {0}")]
    Io(#[from] std::io::Error),

    #[error("Regex error: {0}")]
    Regex(#[from] regex::Error),

    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML serialization error: {0}")]
    TomlSer(#[from] toml::ser::Error),
    
    #[error("TOML deserialization error: {0}")]
    TomlDe(#[from] toml::de::Error),

    #[error("System time error: {0}")]
    SystemTime(#[from] std::time::SystemTimeError),

    #[error("Unknown tool: {0}")]
    UnknownTool(String),

    #[error("{0}")]
    Custom(String),
}

pub type Result<T> = std::result::Result<T, MirrorError>;
