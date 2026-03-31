// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

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
//! use telescope_collector_sdk::{AgentConfig, Collector, CollectorManifest, run};
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
//!         }
//!     }
//!
//!     fn agent(&self) -> AgentConfig {
//!         AgentConfig {
//!             agent_id: "my-agent".into(),
//!             name: "My Agent".into(),
//!             agent_type: "ai-assistant".into(),
//!             version: None,
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
}

/// Agent identity declared by a collector.
///
/// Every collector must declare which agent it serves. The service uses
/// this to create/upsert the agent entity and attribute all events.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Stable key for deterministic agent ID (e.g. "github-copilot").
    pub agent_id: String,
    /// Human-readable display name.
    pub name: String,
    /// Agent type (e.g. "ai-assistant").
    pub agent_type: String,
    /// Optional version string.
    pub version: Option<String>,
}

/// Trait for implementing an out-of-process Telescope collector.
///
/// Implement `manifest()`, `agent()`, `collect()`, and `interval()` at minimum.
/// The SDK's [`run()`] function handles the rest (pipe connection, registration,
/// batching, backpressure, reconnection, graceful shutdown).
#[async_trait::async_trait]
pub trait Collector: Send + 'static {
    /// Return the collector's metadata (used for registration).
    fn manifest(&self) -> CollectorManifest;

    /// Declare which agent this collector serves.
    ///
    /// The service creates or updates the agent entity at registration time.
    /// All events submitted by this collector are attributed to this agent.
    fn agent(&self) -> AgentConfig;

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
