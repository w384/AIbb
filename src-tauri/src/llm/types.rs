use std::time::Duration;

use async_trait::async_trait;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::error::AppError;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::new("user", content)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeWebRequest {
    pub input: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeWebOutcome {
    Completed(String),
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransportTimeouts {
    pub response_headers: Duration,
    pub total_stream: Duration,
}

impl Default for TransportTimeouts {
    fn default() -> Self {
        Self {
            response_headers: Duration::from_secs(20),
            total_stream: Duration::from_secs(90),
        }
    }
}

#[async_trait]
pub trait DeltaSink: Send + Sync {
    async fn send(&self, delta: &str) -> Result<(), AppError>;
}

#[async_trait]
pub trait LlmTransport: Send + Sync {
    async fn stream_chat(
        &self,
        request: ChatRequest,
        sink: &dyn DeltaSink,
        cancellation: CancellationToken,
    ) -> Result<(), AppError>;

    async fn complete(
        &self,
        request: ChatRequest,
        cancellation: CancellationToken,
    ) -> Result<String, AppError>;

    async fn try_native_web(
        &self,
        request: NativeWebRequest,
        cancellation: CancellationToken,
    ) -> Result<NativeWebOutcome, AppError>;

    async fn test_connection(&self, cancellation: CancellationToken) -> Result<(), AppError>;
}
