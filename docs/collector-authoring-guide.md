# Collector Authoring Guide

This guide walks you through building, running, and installing a custom Project Telescope collector. By the end you will have a working "Hello World" collector that emits events into the Telescope pipeline.

## Prerequisites

- **Rust 1.94+** — install from [rustup.rs](https://rustup.rs)
- **Project Telescope service** — installed and running (`tele doctor` to verify)
- A clone of this repo:
  ```bash
  git clone https://github.com/microsoft/project-telescope.git
  cd project-telescope
  ```

## What is a collector?

A collector is a standalone binary that connects to the Telescope service over a local IPC pipe, registers itself, and periodically submits canonical events. The SDK handles all the plumbing — pipe connection, registration, batching, backpressure, reconnection, and graceful shutdown. You just implement three things:

1. **`manifest()`** — who is this collector?
2. **`collect()`** — what events should it emit right now?
3. **`interval()`** — how often should `collect()` be called?

## Project layout

A collector lives in its own directory with this structure:

```
my-collector/
├── Cargo.toml          # Rust package manifest
├── collector.toml      # Telescope collector metadata
└── src/
    └── main.rs         # Collector implementation
```

## Step 1: Create the Cargo project

```toml
# Cargo.toml
[package]
name = "telescope-collector-hello-world"
version = "0.1.0"
edition = "2024"

[[bin]]
name = "telescope-collector-hello-world"
path = "src/main.rs"

[dependencies]
telescope-collector-sdk = { path = "../../src/crates/collector-sdk" }
async-trait = "0.1"
serde_json = "1"
uuid = { version = "1", features = ["v4", "v5"] }
tokio = { version = "1", features = ["full"] }
anyhow = "1"
chrono = { version = "0.4", features = ["serde"] }
```

> **Tip:** If you are developing inside the project-telescope workspace, use `{ workspace = true }` for shared dependencies instead of pinning versions. See `\examples\hello_world\Cargo.toml` for an example to use workspace.

## Step 2: Write the collector manifest

```toml
# collector.toml
[collector]
name = "hello-world"
version = "0.1.0"
description = "A minimal hello world collector."
executable = "telescope-collector-hello-world"
lifecycle = "managed"
author = "Your Name"
```

Key fields:

| Field | Description |
|-------|-------------|
| `name` | Unique identifier for the collector. Used in `tele collector enable <name>`. |
| `version` | Semantic version string. |
| `description` | Human-readable summary. |
| `executable` | Binary name produced by `cargo build`. |
| `lifecycle` | `"managed"` (Telescope starts/stops it) or `"autonomous"` (you manage it). |
| `author` | Who wrote it. |

## Step 3: Implement the `Collector` trait

```rust
// src/main.rs
use std::time::Duration;

use telescope_collector_sdk::{Collector, CollectorManifest, EventKind, ProvenanceConfig};
use uuid::Uuid;

fn agent_id() -> Uuid {
    Uuid::new_v5(&Uuid::NAMESPACE_OID, b"telescope-hello-world-agent")
}

fn session_id() -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("hello-world-session-{}", std::process::id()).as_bytes(),
    )
}

struct HelloWorldCollector {
    tick: u64,
    agent_id: Uuid,
    session_id: Uuid,
    started: bool,
}

impl HelloWorldCollector {
    fn new() -> Self {
        Self {
            tick: 0,
            agent_id: agent_id(),
            session_id: session_id(),
            started: false,
        }
    }
}

#[async_trait::async_trait]
impl Collector for HelloWorldCollector {
    fn manifest(&self) -> CollectorManifest {
        CollectorManifest {
            name: "hello-world".into(),
            version: "0.1.0".into(),
            description: "A minimal hello world collector.".into(),
            provenance: ProvenanceConfig {
                collector_type: "manual".into(),
                capture_method: "volunteered".into(),
            },
        }
    }

    async fn collect(&mut self) -> anyhow::Result<Vec<EventKind>> {
        self.tick += 1;
        let mut events = Vec::new();

        // Bootstrap: emit agent + session discovery on first cycle.
        if !self.started {
            self.started = true;
            events.push(EventKind::AgentDiscovered {
                agent_id: self.agent_id,
                name: "hello-world-agent".into(),
                agent_type: "example".into(),
                executable_path: Some("telescope-collector-hello-world".into()),
                version: Some("0.1.0".into()),
            });
            events.push(EventKind::SessionStarted {
                session_id: self.session_id,
                agent_id: self.agent_id,
                cwd: std::env::current_dir()
                    .ok()
                    .map(|p| p.display().to_string()),
                git_repo: None,
                git_branch: None,
            });
        }

        // Emit a custom hello event every cycle.
        events.push(EventKind::Custom {
            event_type: "hello_world".into(),
            data: serde_json::json!({
                "message": format!("Hello from Telescope! (tick #{})", self.tick),
                "agent_id": self.agent_id.to_string(),
                "session_id": self.session_id.to_string(),
            }),
        });

        Ok(events)
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(15)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    telescope_collector_sdk::run(HelloWorldCollector::new()).await
}
```

### What this does

1. **First cycle**: emits `AgentDiscovered` and `SessionStarted` to register the agent and session with Telescope.
2. **Every cycle** (every 15 seconds): emits a `Custom` event with a hello message and a tick counter.
3. **Shutdown**: the SDK handles Ctrl-C / SIGTERM and calls `stop()`.

### Provenance

Every collector declares its provenance — where the data came from and how it was captured:

| Field | Options | Description |
|-------|---------|-------------|
| `collector_type` | `mcp_proxy`, `copilot_sdk`, `session_log`, `process_scan`, `self_report`, `manual`, ... | How the data was obtained |
| `capture_method` | `live_intercept`, `live_sdk_hook`, `post_hoc_log_parse`, `snapshot`, `volunteered`, `inferred` | When/how the data was captured |

For a hello world example, `manual` / `volunteered` is appropriate because the collector is self-reporting synthetic data.

## Step 4: Build

```bash
cargo build --release -p telescope-collector-hello-world
```

The binary lands in `target/release/telescope-collector-hello-world` (or `.exe` on Windows).

## Step 5: Install and run

```bash
# Install the collector 
tele collector install ./target/release/telescope-collector-hello-world/

# Enable it so the service starts managing it
tele collectors enable hello-world

# Verify it's loaded
tele collector list
tele collector info hello-world
```

To see events flowing:

```bash
tele watch
```

You should see `AgentDiscovered`, `SessionStarted`, and periodic `hello_world` custom events.

## Step 6: Iterate

To rebuild and reinstall after making changes:

```bash
cargo build --release -p telescope-collector-hello-world
tele collector install ./examples/hello_world/
```

The service will pick up the new binary automatically.

## Canonical event types

Your collector can emit any of the ~40 canonical event types. The most commonly used ones:

| Event | When to use |
|-------|-------------|
| `AgentDiscovered` | First time you see an agent |
| `SessionStarted` / `SessionEnded` | Bracket a work session |
| `UserMessage` | The human said something |
| `TurnStarted` / `TurnCompleted` | The agent is thinking |
| `ToolCallStarted` / `ToolCallCompleted` | The agent called a tool |
| `FileRead` / `FileWritten` | File I/O |
| `ShellCommandStarted` / `ShellCommandCompleted` | Shell execution |
| `ErrorOccurred` | Something went wrong |
| `TokenUsageReported` | Cost tracking |
| `Custom { event_type, data }` | Anything else |

See `src/crates/collector-types/src/canonical/events.rs` for the full list.


## Next steps

- Browse `src/collectors/heartbeat/` for the simplest built-in collector.
- Browse `src/collectors/copilot-jsonl/` for a real-world file-based collector with incremental scanning.