#[derive(Debug, thiserror::Error)]
pub enum VigilError {
    #[error("session not found: {0}")]
    SessionNotFound(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("parse error in {context}: {message}")]
    Parse { context: String, message: String },

    #[error("process probe failed: {0}")]
    ProcessProbe(String),

    #[error("not supported: {0}")]
    NotSupported(String),
}
