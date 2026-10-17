use thiserror::Error;

/// Cache-specific error types
#[derive(Debug, Error)]
pub enum CacheError {
    #[error("Cache worker failed: {0}")]
    WorkerError(#[from] tokio::task::JoinError),

    /// Failed to acquire a connection from the pool
    #[error("Failed to get connection from pool: {0}")]
    ConnectionPoolError(#[from] r2d2::Error),

    /// Database operation failed
    #[error("Database operation failed: {0}")]
    DatabaseError(#[from] rusqlite::Error),

    /// JSON serialization/deserialization failed
    #[error("JSON serialization failed: {0}")]
    SerializationError(#[from] serde_json::Error),

    /// System time error
    #[error("System time error: {0}")]
    SystemTimeError(#[from] std::time::SystemTimeError),

    /// Invalid input data (e.g., empty vectors)
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// IO error during cache operations
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

pub type CacheResult<T> = Result<T, CacheError>;
