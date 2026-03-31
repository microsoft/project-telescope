// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//! Protocol types for the collector IPC channel.
//!
//! Out-of-process collectors connect to `telescope-collector` and use these
//! types to register, submit canonical events, send heartbeats, and deregister.

use serde::{Deserialize, Serialize};

/// Maximum events per `collector.submit` call.
pub const MAX_BATCH_SIZE: u32 = 500;

// ── collector.register ──

/// Agent identity declared by a collector at registration time.
///
/// Every collector must know which agent it is collecting for and declare
/// it upfront. The service uses this to create/upsert the agent entity
/// and attribute all events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    /// Stable key for deterministic `AgentId` generation (e.g., "github-copilot").
    pub agent_id: String,
    /// Human-readable display name (e.g., "GitHub Copilot").
    pub name: String,
    /// Agent type classification (e.g., "ai-assistant").
    pub agent_type: String,
    /// Agent version string, if known.
    pub version: Option<String>,
}

/// Parameters for `collector.register`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterParams {
    /// Collector name (must be unique across connected collectors).
    pub name: String,
    /// Semantic version of the collector.
    pub version: String,
    /// Human-readable description.
    pub description: String,
    /// The agent this collector is collecting for (required).
    pub agent: AgentInfo,
    /// OS process ID of the collector.
    pub pid: u32,
    /// Expected interval between collect cycles (seconds). Used for health monitoring.
    pub expected_interval_secs: Option<u64>,
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
    /// Number of events accepted.
    pub accepted: u32,
    /// Suggested delay (ms) before next submit.
    pub delay_hint_ms: u64,
}

// ── Backpressure ──

/// Compute the delay hint based on the event backlog.
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
