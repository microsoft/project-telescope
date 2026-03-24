//! Telescope Collector SDK — build out-of-process collectors for Telescope.
#![allow(unsafe_code)]
//!
//! Provides two APIs:
//!
//! - **Rust API**: Implement the [`Collector`] trait and call [`run()`] — the SDK handles
//!   pipe connection, registration, collect loops, batching, backpressure, reconnection,
//!   and graceful shutdown.
//!
//! - **C-ABI**: Link against the cdylib and call `telescope_sdk_init` / `telescope_sdk_submit` /
//!   `telescope_sdk_shutdown` for non-Rust collectors (C, Python, Go, etc.). See the C header
//!   at `include/telescope_collector_sdk.h`.
//!
//! # Quick Start (Rust)
//!
//! ```rust,no_run
//! use telescope_collector_sdk::{Collector, CollectorManifest, ProvenanceConfig, run};
//! use telescope_collector_types::canonical::events::EventKind;
//! use std::time::Duration;
//!
//! struct MyCollector;
//!
//! #[async_trait::async_trait]
//! impl Collector for MyCollector {
//!     fn manifest(&self) -> CollectorManifest {
//!         CollectorManifest {
//!             name: "my-collector".into(),
//!             version: "0.1.0".into(),
//!             description: "My custom collector".into(),
//!             provenance: ProvenanceConfig {
//!                 collector_type: "session_log".into(),
//!                 capture_method: "post_hoc_log_parse".into(),
//!             },
//!         }
//!     }
//!
//!     async fn collect(&mut self) -> anyhow::Result<Vec<EventKind>> {
//!         Ok(vec![])
//!     }
//!
//!     fn interval(&self) -> Duration {
//!         Duration::from_secs(15)
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     run(MyCollector).await
//! }
//! ```

mod ffi;
mod runtime;

pub use telescope_collector_types::canonical::EventKind;

use std::time::Duration;

/// Metadata about a collector, sent during registration.
#[derive(Debug, Clone)]
pub struct CollectorManifest {
    /// Unique collector name.
    pub name: String,
    /// Semantic version.
    pub version: String,
    /// Human-readable description.
    pub description: String,
    /// Provenance configuration for events from this collector.
    pub provenance: ProvenanceConfig,
}

/// Provenance settings for a collector.
#[derive(Debug, Clone)]
pub struct ProvenanceConfig {
    /// Maps to a `CollectorType` variant name (e.g. `"session_log"`).
    pub collector_type: String,
    /// Maps to a `CaptureMethod` variant name (e.g. `"post_hoc_log_parse"`).
    pub capture_method: String,
}

/// Trait for implementing an out-of-process Telescope collector.
///
/// Implement `manifest()`, `collect()`, and `interval()` at minimum.
/// The SDK's [`run()`] function handles the rest (pipe connection, registration,
/// batching, backpressure, reconnection, graceful shutdown).
#[async_trait::async_trait]
pub trait Collector: Send + 'static {
    /// Return the collector's metadata (used for registration).
    fn manifest(&self) -> CollectorManifest;

    /// Called once after the first successful connection.
    async fn start(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Collect events. Called every [`interval()`](Collector::interval).
    /// Return an empty vec if there's nothing to report.
    async fn collect(&mut self) -> anyhow::Result<Vec<EventKind>>;

    /// How often to call `collect()`.
    fn interval(&self) -> Duration;

    /// Called during graceful shutdown.
    async fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Run a collector as a standalone process.
///
/// Connects to the Telescope service's collector IPC channel, registers,
/// and enters a collect/submit loop. Handles:
/// - Pipe connection with exponential backoff retry
/// - Registration
/// - Collect loop at the configured interval
/// - Event batching (max 500 per submit)
/// - Backpressure compliance (`delay_hint_ms`)
/// - Heartbeat every 30s if no submits
/// - Reconnection on disconnect
/// - Graceful shutdown on SIGTERM/Ctrl-C
pub async fn run(collector: impl Collector) -> anyhow::Result<()> {
    runtime::run_collector(collector).await
}
