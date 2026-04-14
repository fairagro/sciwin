use thiserror::Error;

#[derive(Debug, Error)]
pub enum RocrateError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Invalid entity ID: {0}")]
    InvalidId(String),
    #[error("Missing required field: {0}")]
    MissingField(String),
}