use std::error::Error;
use std::fmt;

/// Errors that can occur during RPC handling
#[derive(Debug)]
pub enum RpcError {
    /// Failed to deserialize input (e.g., invalid Cap’n Proto message)
    Deserialization(String),

    /// Failed to serialize output (e.g., invalid Cap’n Proto builder)
    Serialization(String),

    /// Application-level error (your handler returned an error)
    Application(String),

    /// Generic catch-all
    Other(String),
}

impl fmt::Display for RpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RpcError::Deserialization(e) => write!(f, "Deserialization error: {}", e),
            RpcError::Serialization(e) => write!(f, "Serialization error: {}", e),
            RpcError::Application(e) => write!(f, "Application error: {}", e),
            RpcError::Other(e) => write!(f, "Other error: {}", e),
        }
    }
}

impl Error for RpcError {}
