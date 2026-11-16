use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Stable error codes shared by HTTP, CLI, MCP and FFI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    ValidationFailed,
    NotFound,
    RevisionConflict,
    PolicyChanged,
    PermissionDenied,
    ApprovalRequired,
    ApprovalExpired,
    IdempotencyMismatch,
    ResourceExhausted,
    RunNotFound,
    ProviderUnavailable,
    UnsupportedOperation,
    StorageFailure,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct ZodoError {
    pub code: ErrorCode,
    pub message: String,
    pub details: Value,
    pub retryable: bool,
}

impl ZodoError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: Value::Null,
            retryable: false,
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }

    pub fn retryable(mut self) -> Self {
        self.retryable = true;
        self
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::ValidationFailed, message)
    }

    pub fn not_found(kind: &str, id: &str) -> Self {
        Self::new(ErrorCode::NotFound, format!("{kind} {id:?} was not found"))
            .with_details(serde_json::json!({"kind": kind, "id": id}))
    }

    pub fn conflict(expected: u64, current: u64) -> Self {
        Self::new(
            ErrorCode::RevisionConflict,
            "the facts this request was based on have changed",
        )
        .with_details(serde_json::json!({
            "expected_revision": expected,
            "current_revision": current,
            "recovery": "refresh_and_repropose"
        }))
    }
}

pub type Result<T> = std::result::Result<T, ZodoError>;

pub fn utc_now() -> DateTime<Utc> {
    Utc::now()
}
