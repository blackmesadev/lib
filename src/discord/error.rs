use thiserror::Error;

use crate::{
    cache::{MemoryCacheError, RedisCacheError},
    db::DbError,
};

#[derive(Error, Debug)]
pub enum DiscordError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Middleware error: {0}")]
    Middleware(#[from] reqwest_middleware::Error),

    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("Voice error: {0}")]
    Voice(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Gateway not connected")]
    NotConnected,

    /// Discord sent op 7 (RECONNECT) - caller should try to RESUME.
    #[error("Discord requested reconnection")]
    Reconnect,

    /// Discord sent op 9 (INVALID_SESSION).
    /// `true`  = session is resumable (try RESUME).
    /// `false` = session is gone (must re-IDENTIFY).
    #[error("Invalid session (resumable={0})")]
    InvalidSession(bool),

    #[error("Gateway connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Invalid payload received: {0}")]
    InvalidPayload(String),

    #[error("In memory cache error: {0}")]
    MemoryCacheError(#[from] MemoryCacheError),

    #[error("Redis cache error: {0}")]
    RedisCacheError(#[from] RedisCacheError),

    #[error("Database error: {0}")]
    DbError(#[from] DbError),

    #[error("Key not found: {0}")]
    KeyNotFound(String),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("{0}")]
    Other(String),
}

pub type DiscordResult<T> = Result<T, DiscordError>;
