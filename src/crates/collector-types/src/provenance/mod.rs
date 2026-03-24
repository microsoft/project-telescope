//! Provenance model — tracks data origin, capture method, and confidence.

use serde::{Deserialize, Serialize};

/// Attached to every entity to track where the data came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    /// Which collector produced this data.
    pub collector_type: CollectorType,
    /// Unique identifier for the collector instance that produced this.
    pub source_id: String,
    /// How confident we are in this data (0.0–1.0).
    pub confidence: f64,
    /// How the data was captured.
    pub capture_method: CaptureMethod,
    /// If corroborated by another source, reference it.
    pub corroborated_by: Option<Box<Provenance>>,
}

impl Provenance {
    /// Create a minimal provenance for the given collector with its default confidence.
    #[must_use]
    pub fn new(collector_type: CollectorType, source_id: String) -> Self {
        let (confidence, capture_method) = match &collector_type {
            CollectorType::McpProxy => (0.95, CaptureMethod::LiveIntercept),
            CollectorType::CopilotSdk => (0.92, CaptureMethod::LiveSdkHook),
            CollectorType::OsKernel => (0.90, CaptureMethod::LiveKernelEvent),
            CollectorType::SessionLog => (0.80, CaptureMethod::PostHocLogParse),
            CollectorType::ProcessScan => (0.60, CaptureMethod::Snapshot),
            CollectorType::SelfReport => (0.55, CaptureMethod::Volunteered),
            CollectorType::Bridge { .. } => (0.90, CaptureMethod::LiveIntercept),
            CollectorType::Manual => (0.30, CaptureMethod::Volunteered),
        };
        Self {
            collector_type,
            source_id,
            confidence,
            capture_method,
            corroborated_by: None,
        }
    }

    /// Boost confidence by corroborating with another provenance source.
    /// Uses the formula: `min(1.0, self.confidence + other.confidence × 0.5)`.
    pub fn corroborate(&mut self, other: Provenance) {
        self.confidence = (self.confidence + other.confidence * 0.5).min(1.0);
        self.corroborated_by = Some(Box::new(other));
    }

    /// Compute inherited confidence for bridge-forwarded data.
    /// Original confidence × 0.90 for the network hop.
    #[must_use]
    pub fn bridge_inherited_confidence(&self) -> f64 {
        self.confidence * 0.90
    }
}

/// Which collector produced this data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CollectorType {
    /// MCP stdio proxy — intercepting live JSON-RPC traffic.
    McpProxy,
    /// OS-level kernel tracing (ETW, eBPF, Endpoint Security).
    OsKernel,
    /// Agent self-reporting via MCP tools.
    SelfReport,
    /// Session log file import (Copilot `events.jsonl`, Claude logs).
    SessionLog,
    /// Process scanner (snapshot of running processes).
    ProcessScan,
    /// Copilot SDK hooks — live event stream via hooks.json + SDK event API.
    CopilotSdk,
    /// Cross-device bridge (forwarded from another Telescope instance).
    Bridge {
        /// The device that forwarded this data.
        device_id: String,
    },
    /// User manual entry.
    Manual,
}

impl CollectorType {
    /// Serialize to a string tag for `SQLite` storage.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::McpProxy => "mcp_proxy",
            Self::OsKernel => "os_kernel",
            Self::SelfReport => "self_report",
            Self::SessionLog => "session_log",
            Self::ProcessScan => "process_scan",
            Self::CopilotSdk => "copilot_sdk",
            Self::Bridge { .. } => "bridge",
            Self::Manual => "manual",
        }
    }
}

/// How the data was captured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptureMethod {
    /// Real-time interception of live traffic.
    LiveIntercept,
    /// Real-time kernel/OS event subscription.
    LiveKernelEvent,
    /// Real-time SDK hook callbacks (Copilot SDK event stream).
    LiveSdkHook,
    /// Parsing a log file after the fact.
    PostHocLogParse,
    /// One-time snapshot (e.g., process list scan).
    Snapshot,
    /// Agent volunteered this information.
    Volunteered,
    /// Inferred from heuristics.
    Inferred,
}

impl CaptureMethod {
    /// Serialize to a string tag for `SQLite` storage.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::LiveIntercept => "live_intercept",
            Self::LiveKernelEvent => "live_kernel_event",
            Self::LiveSdkHook => "live_sdk_hook",
            Self::PostHocLogParse => "post_hoc_log_parse",
            Self::Snapshot => "snapshot",
            Self::Volunteered => "volunteered",
            Self::Inferred => "inferred",
        }
    }

    /// Parse from a string tag stored in `SQLite`.
    #[must_use]
    pub fn from_str_tag(s: &str) -> Self {
        match s {
            "live_intercept" => Self::LiveIntercept,
            "live_kernel_event" => Self::LiveKernelEvent,
            "live_sdk_hook" => Self::LiveSdkHook,
            "post_hoc_log_parse" => Self::PostHocLogParse,
            "snapshot" => Self::Snapshot,
            "volunteered" => Self::Volunteered,
            _ => Self::Inferred,
        }
    }
}
