//! Protocol types for the collector IPC channel.
//!
//! Out-of-process collectors connect to `telescope-collector` and use these
//! types to register, submit canonical events, send heartbeats, and deregister.

use serde::{Deserialize, Serialize};

/// Maximum events per `collector.submit` call.
pub const MAX_BATCH_SIZE: u32 = 500;

/// Backpressure threshold: hard-reject above this many unpromoted events.
pub const BACKPRESSURE_REJECT_THRESHOLD: u64 = 100_000;

// ── collector.register ──

/// Parameters for `collector.register`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterParams {
    /// Collector name (must be unique across connected collectors).
    pub name: String,
    /// Semantic version.
    pub version: String,
    /// Human-readable description.
    pub description: String,
    /// Provenance configuration for events from this collector.
    pub provenance: ProvenanceParams,
    /// OS process ID of the collector.
    pub pid: u32,
    /// Expected interval between collect cycles (seconds). Used for health monitoring.
    pub expected_interval_secs: Option<u64>,
}

/// Provenance parameters supplied at registration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceParams {
    /// Maps to [`CollectorType`](crate::provenance::CollectorType) variant name.
    pub collector_type: String,
    /// Maps to [`CaptureMethod`](crate::provenance::CaptureMethod) variant name.
    pub capture_method: String,
}

/// Successful response to `collector.register`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterResponse {
    /// Always `"registered"`.
    pub status: String,
    /// The collector ID assigned by the service (same as `name`).
    pub collector_id: String,
    /// Maximum events per submit batch.
    pub max_batch_size: u32,
}

// ── collector.submit ──

/// Parameters for `collector.submit`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitParams {
    /// Array of `EventKind` values serialized as JSON objects.
    pub events: Vec<serde_json::Value>,
}

/// Successful response to `collector.submit`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitResponse {
    /// Number of events accepted and inserted into the canonical store.
    pub accepted: u32,
    /// Suggested delay (ms) before next submit. Based on canonical store backlog.
    pub delay_hint_ms: u64,
}

// ── Backpressure ──

/// Compute the delay hint based on the unpromoted event backlog.
///
/// | Backlog          | Delay (ms) | Meaning              |
/// |------------------|------------|----------------------|
/// | < 5,000          | 0          | Normal               |
/// | 5,000 – 19,999   | 1,000      | Mild backpressure    |
/// | 20,000 – 49,999  | 5,000      | Heavy backpressure   |
/// | 50,000 – 99,999  | 10,000     | Critical backpressure|
/// | ≥ 100,000        | N/A        | Hard reject (-32006) |
#[must_use]
pub fn compute_delay_hint(backlog: u64) -> u64 {
    match backlog {
        0..5_000 => 0,
        5_000..20_000 => 1_000,
        20_000..50_000 => 5_000,
        50_000..100_000 => 10_000,
        _ => u64::MAX, // caller should hard-reject before reaching here
    }
}

// ── Provenance parsing ──

/// Parse a `collector_type` string into a [`CollectorType`](crate::provenance::CollectorType).
///
/// Returns `None` for unrecognized strings.
#[must_use]
pub fn parse_collector_type(s: &str) -> Option<crate::provenance::CollectorType> {
    use crate::provenance::CollectorType;
    match s {
        "mcp_proxy" | "McpProxy" => Some(CollectorType::McpProxy),
        "os_kernel" | "OsKernel" => Some(CollectorType::OsKernel),
        "self_report" | "SelfReport" => Some(CollectorType::SelfReport),
        "session_log" | "SessionLog" => Some(CollectorType::SessionLog),
        "process_scan" | "ProcessScan" => Some(CollectorType::ProcessScan),
        "copilot_sdk" | "CopilotSdk" => Some(CollectorType::CopilotSdk),
        "manual" | "Manual" => Some(CollectorType::Manual),
        _ => None,
    }
}

/// Parse a `capture_method` string into a [`CaptureMethod`](crate::provenance::CaptureMethod).
///
/// Returns `None` for unrecognized strings.
#[must_use]
pub fn parse_capture_method(s: &str) -> Option<crate::provenance::CaptureMethod> {
    use crate::provenance::CaptureMethod;
    match s {
        "live_intercept" | "LiveIntercept" => Some(CaptureMethod::LiveIntercept),
        "live_kernel_event" | "LiveKernelEvent" => Some(CaptureMethod::LiveKernelEvent),
        "live_sdk_hook" | "LiveSdkHook" => Some(CaptureMethod::LiveSdkHook),
        "post_hoc_log_parse" | "PostHocLogParse" => Some(CaptureMethod::PostHocLogParse),
        "snapshot" | "Snapshot" => Some(CaptureMethod::Snapshot),
        "volunteered" | "Volunteered" => Some(CaptureMethod::Volunteered),
        "inferred" | "Inferred" => Some(CaptureMethod::Inferred),
        _ => None,
    }
}
