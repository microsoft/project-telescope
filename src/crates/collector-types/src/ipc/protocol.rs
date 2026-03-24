//! IPC protocol — request/response types.
//!
//! Simple JSON-RPC-inspired protocol: `{ "method": "...", "params": {...} }`
//! with `{ "result": ... }` or `{ "error": { "code": ..., "message": "..." } }`.
//!
//! An [`IpcNotification`] is a server-pushed message sent on subscription
//! connections (no response expected from the client).

use serde::{Deserialize, Serialize};

/// An IPC request from CLI or reader to the service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcRequest {
    /// Method name (e.g. `"list_collectors"`, `"enable_collector"`).
    pub method: String,
    /// Parameters as a JSON value (object or null).
    #[serde(default)]
    pub params: serde_json::Value,
}

/// An IPC response from the service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcResponse {
    /// Success payload (present on success).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// Error payload (present on failure).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<IpcError>,
}

/// An IPC error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcError {
    /// Machine-readable error code.
    pub code: i32,
    /// Human-readable error message.
    pub message: String,
}

/// Well-known error codes.
pub mod error_codes {
    // ── Standard JSON-RPC error codes ────────────────────────────────────

    /// Method not found.
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// Invalid parameters.
    pub const INVALID_PARAMS: i32 = -32602;
    /// Internal server error.
    pub const INTERNAL_ERROR: i32 = -32603;

    // ── Telescope-specific error codes (-32000 to -32099) ────────────────

    /// The requested collector was not found in the registry.
    pub const COLLECTOR_NOT_FOUND: i32 = -32001;
    /// The collector exists but is not currently running.
    pub const COLLECTOR_NOT_RUNNING: i32 = -32002;
    /// The requested session was not found.
    pub const SESSION_NOT_FOUND: i32 = -32003;
    /// The requested agent was not found.
    pub const AGENT_NOT_FOUND: i32 = -32004;
    /// The collector is already registered on this connection.
    pub const COLLECTOR_ALREADY_REGISTERED: i32 = -32005;
    /// Backpressure overload — too many unpromoted events in the canonical store.
    pub const BACKPRESSURE_OVERLOAD: i32 = -32006;
    /// The collector must register before submitting events.
    pub const REGISTRATION_REQUIRED: i32 = -32007;
    /// A store (`SQLite`) query or write failed.
    pub const STORE_ERROR: i32 = -32010;
    /// Serialization or deserialization failed.
    pub const SERIALIZATION_ERROR: i32 = -32011;
}

impl IpcRequest {
    /// Create a new request.
    pub fn new(method: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            method: method.into(),
            params,
        }
    }

    /// Create a request with no parameters.
    pub fn simple(method: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            params: serde_json::Value::Null,
        }
    }
}

impl IpcResponse {
    /// Create a success response.
    pub fn success(value: serde_json::Value) -> Self {
        Self {
            result: Some(value),
            error: None,
        }
    }

    /// Create an error response.
    pub fn error(code: i32, message: impl Into<String>) -> Self {
        Self {
            result: None,
            error: Some(IpcError {
                code,
                message: message.into(),
            }),
        }
    }

    /// Check if this response is an error.
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }
}

/// A server-pushed notification sent on subscription connections.
///
/// Unlike an [`IpcResponse`], a notification carries a method name and
/// parameters and does **not** expect a response from the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcNotification {
    /// Notification type (e.g. `"entity_update"`).
    pub method: String,
    /// Payload as a JSON value.
    #[serde(default)]
    pub params: serde_json::Value,
}

impl IpcNotification {
    /// Create a new notification.
    pub fn new(method: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            method: method.into(),
            params,
        }
    }
}
