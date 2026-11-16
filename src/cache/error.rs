use thiserror::Error;

/// Cache-specific error types
#[derive(Debug, Error)]
pub enum CacheError {
    #[error("Cache worker failed")]
    WorkerError(#[from] tokio::task::JoinError),

    /// Failed to acquire a connection from the pool
    #[error("Failed to get connection from pool")]
    ConnectionPoolError(#[from] r2d2::Error),

    /// Database operation failed
    #[error("Database operation failed")]
    DatabaseError(#[from] rusqlite::Error),

    /// JSON serialization/deserialization failed
    #[error("JSON serialization failed")]
    SerializationError(#[from] serde_json::Error),

    /// System time error
    #[error("System time error")]
    SystemTimeError(#[from] std::time::SystemTimeError),

    /// Invalid input data
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// IO error during cache operations
    #[error("IO error")]
    IoError(#[from] std::io::Error),
}

pub type CacheResult<T> = Result<T, CacheError>;
