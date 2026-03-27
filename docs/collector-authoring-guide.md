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

## Event JSON format

Events are serialized as JSON using an internally tagged representation. Every event object has a `"type"` field that identifies the variant, with all other fields alongside it. Optional fields are omitted when `null`.

### Serialization rules

| Type | JSON representation | Example |
|------|-------------------|---------|
| UUID | Standard string | `"550e8400-e29b-41d4-a716-446655440000"` |
| DateTime | ISO 8601 UTC | `"2024-01-15T10:30:45.123456Z"` |
| Optional field | Omitted if None | field absent from object |
| Enum variant | `snake_case` string | `"agent_discovered"` |

### Wire protocol

Collectors communicate with the Telescope service over local IPC (named pipes on Windows, Unix sockets on Linux/macOS) using a length-prefixed frame protocol:

```
[4-byte little-endian length][JSON payload]
```

Maximum frame size is 16 MiB. The SDK handles framing automatically — you only work with `EventKind` values.

### IPC message flow

**1. Registration** — the SDK sends this automatically from your `manifest()`:

```json
{
  "method": "collector.register",
  "params": {
    "name": "hello-world",
    "version": "0.1.0",
    "description": "A minimal hello world collector.",
    "provenance": {
      "collector_type": "manual",
      "capture_method": "volunteered"
    },
    "pid": 12345,
    "expected_interval_secs": 15
  }
}
```

Response:

```json
{
  "result": {
    "status": "registered",
    "collector_id": "hello-world",
    "max_batch_size": 500
  }
}
```

**2. Event submission** — the SDK batches your `collect()` return values and sends them:

```json
{
  "method": "collector.submit",
  "params": {
    "events": [
      { "type": "agent_discovered", "agent_id": "...", "name": "..." },
      { "type": "session_started", "session_id": "...", "agent_id": "..." }
    ]
  }
}
```

Response:

```json
{
  "result": {
    "accepted": 2,
    "delay_hint_ms": 0
  }
}
```

The `delay_hint_ms` field signals backpressure — the SDK automatically waits when the service is overloaded. Maximum batch size is 500 events.

### Event reference

Below is the JSON shape for every canonical event type, grouped by category. **Required** fields are always present; **optional** fields are omitted when not set.

#### Agent events

```json
// AgentDiscovered — register a new agent
{
  "type": "agent_discovered",
  "agent_id": "550e8400-e29b-41d4-a716-446655440000",  // required, UUID
  "name": "my-agent",                                    // required
  "agent_type": "copilot",                               // required
  "executable_path": "/usr/bin/my-agent",                // optional
  "version": "1.0.0"                                     // optional
}

// AgentHeartbeat — signal agent is alive
{
  "type": "agent_heartbeat",
  "agent_id": "550e8400-e29b-41d4-a716-446655440000"    // required, UUID
}
```

#### Session events

```json
// SessionStarted
{
  "type": "session_started",
  "session_id": "...",   // required, UUID
  "agent_id": "...",     // required, UUID
  "cwd": "/home/user/project",      // optional
  "git_repo": "owner/repo",         // optional
  "git_branch": "main"              // optional
}

// SessionEnded
{
  "type": "session_ended",
  "session_id": "...",       // required, UUID
  "status": "completed",    // required: "completed", "failed", "cancelled"
  "duration_ms": 120000     // optional
}

// SessionResumed
{
  "type": "session_resumed",
  "session_id": "..."       // required, UUID
}

// SessionMetadataUpdated
{
  "type": "session_metadata_updated",
  "session_id": "...",       // required, UUID
  "metadata": { }           // required, arbitrary JSON object
}

// SessionModeChanged
{
  "type": "session_mode_changed",
  "session_id": "...",           // required, UUID
  "previous_mode": "interactive", // required
  "new_mode": "plan"             // required
}
```

#### Turn events

```json
// UserMessage
{
  "type": "user_message",
  "session_id": "...",   // required, UUID
  "turn_id": "...",      // required, UUID
  "content": "What does this function do?"  // optional
}

// TurnStarted
{
  "type": "turn_started",
  "session_id": "...",    // required, UUID
  "turn_id": "...",       // required, UUID
  "turn_index": 0,        // required, 0-based counter
  "model_name": "gpt-4"   // optional
}

// TurnCompleted
{
  "type": "turn_completed",
  "session_id": "...",
  "turn_id": "...",
  "user_message": "...",            // optional
  "assistant_response": "...",      // optional
  "model_name": "gpt-4",           // optional
  "tokens": {                       // optional, arbitrary JSON
    "input_tokens": 150,
    "output_tokens": 320
  },
  "duration_ms": 5000,              // optional
  "status": "completed"             // required: "completed", "failed"
}

// TurnStreaming
{
  "type": "turn_streaming",
  "turn_id": "...",                  // required, UUID
  "partial_content": "The function...", // optional
  "tokens_so_far": 45               // optional
}
```

#### Tool events

```json
// ToolCallStarted
{
  "type": "tool_call_started",
  "turn_id": "...",       // required, UUID
  "effect_id": "...",     // required, UUID — unique invocation ID
  "name": "search_code",  // required
  "arguments": { }        // optional, arbitrary JSON input
}

// ToolCallCompleted
{
  "type": "tool_call_completed",
  "effect_id": "...",       // required, UUID — matches ToolCallStarted
  "status": "succeeded",    // required: "succeeded", "failed"
  "result": { },            // optional, arbitrary JSON output
  "duration_ms": 1200       // optional
}
```

#### File I/O events

All file events share the same shape:

```json
// FileRead, FileWritten, FileCreated, FileDeleted
{
  "type": "file_read",        // or "file_written", "file_created", "file_deleted"
  "turn_id": "...",            // optional, UUID
  "path": "/src/main.rs"      // required
}
```

#### Shell command events

```json
// ShellCommandStarted
{
  "type": "shell_command_started",
  "turn_id": "...",                    // optional, UUID
  "effect_id": "...",                  // required, UUID
  "command": "cargo build --release",  // required
  "cwd": "/home/user/project"         // optional
}

// ShellCommandCompleted
{
  "type": "shell_command_completed",
  "effect_id": "...",       // required, UUID — matches ShellCommandStarted
  "exit_code": 0,           // optional
  "duration_ms": 45000      // optional
}
```

#### Sub-agent events

```json
// SubAgentSpawned
{
  "type": "sub_agent_spawned",
  "turn_id": "...",                  // optional, UUID
  "effect_id": "...",                // required, UUID
  "agent_type": "search_specialist", // required
  "prompt": "Find all references"    // optional
}

// SubAgentCompleted
{
  "type": "sub_agent_completed",
  "effect_id": "...",       // required, UUID
  "status": "succeeded",    // required
  "duration_ms": 8000       // optional
}
```

#### Planning and reasoning events

```json
// PlanCreated
{
  "type": "plan_created",
  "turn_id": "...",                          // optional, UUID
  "content": "1. Search\n2. Analyze\n3. Fix" // required
}

// PlanStepCompleted
{
  "type": "plan_step_completed",
  "turn_id": "...",                // optional, UUID
  "step": "Search for usages"     // required
}

// ThinkingBlock
{
  "type": "thinking_block",
  "turn_id": "...",                          // optional, UUID
  "content": "I need to consider backwards compatibility..." // required
}
```

#### Context window events

```json
// ContextWindowSnapshot
{
  "type": "context_window_snapshot",
  "session_id": "...",       // required, UUID
  "total_tokens": 45000,     // required
  "max_tokens": 100000       // required
}

// ContextPruned
{
  "type": "context_pruned",
  "session_id": "...",        // required, UUID
  "tokens_removed": 5000     // required
}
```

#### Human-in-the-loop events

```json
// ApprovalRequested
{
  "type": "approval_requested",
  "turn_id": "...",                  // optional, UUID
  "action": "Delete 5 files in /tmp" // required
}

// ApprovalGranted
{
  "type": "approval_granted",
  "turn_id": "..."                   // optional, UUID
}

// ApprovalDenied
{
  "type": "approval_denied",
  "turn_id": "...",                  // optional, UUID
  "reason": "Unsafe operation"       // optional
}

// UserFeedback
{
  "type": "user_feedback",
  "turn_id": "...",                  // optional, UUID
  "content": "Looks good",          // required
  "sentiment": "positive"           // optional: "positive", "negative", "neutral"
}
```

#### Self-report events

```json
// IntentDeclared
{ "type": "intent_declared", "turn_id": "...", "intent": "Implement caching" }

// DecisionMade
{
  "type": "decision_made",
  "turn_id": "...",
  "decision": "Use Redis",
  "reasoning": "Fast, supports TTL",                 // optional
  "alternatives": ["Memcached", "Local in-memory"]   // optional
}

// ThoughtLogged
{ "type": "thought_logged", "turn_id": "...", "content": "This is O(n²)", "category": "optimization" }

// FrustrationReported
{ "type": "frustration_reported", "turn_id": "...", "issue": "Docs outdated", "severity": "high" }

// OutcomeReported
{ "type": "outcome_reported", "turn_id": "...", "outcome": "Refactored 12 functions", "success": true }

// ObservationLogged
{ "type": "observation_logged", "turn_id": "...", "observation": "All tests pass" }

// AssumptionMade
{ "type": "assumption_made", "turn_id": "...", "assumption": "DB returns sorted results" }

// PathNotTaken
{ "type": "path_not_taken", "turn_id": "...", "path": "ML approach", "reason": "Insufficient data" }

// RecipeFollowed
{ "type": "recipe_followed", "turn_id": "...", "recipe": "add_logging_to_function" }
```

#### Model events

```json
// ModelUsed
{
  "type": "model_used",
  "session_id": "...",        // required, UUID
  "name": "gpt-4-turbo",     // required
  "provider": "OpenAI",      // optional
  "tokens": { "input_tokens": 2500, "output_tokens": 800 },  // optional
  "cost": { "currency": "USD", "amount": 0.084 },            // optional
  "invocation_count": 3      // optional
}

// ModelSwitched
{
  "type": "model_switched",
  "session_id": "...",            // required, UUID
  "from_model": "gpt-3.5-turbo", // required
  "to_model": "gpt-4-turbo"      // required
}
```

#### Error events

```json
// ErrorOccurred
{
  "type": "error_occurred",
  "turn_id": "...",                          // optional, UUID
  "session_id": "...",                       // optional, UUID
  "message": "File not found: config.json",  // required
  "category": "file_system"                  // optional
}

// RetryAttempted
{
  "type": "retry_attempted",
  "turn_id": "...",                  // optional, UUID
  "attempt": 2,                      // required
  "reason": "Timeout on API call"    // optional
}
```

#### Code events

```json
// SearchPerformed
{
  "type": "search_performed",
  "turn_id": "...",                 // optional, UUID
  "query": "function getUserById",  // required
  "result_count": 12                // optional
}

// CodeChangeApplied
{
  "type": "code_change_applied",
  "turn_id": "...",              // optional, UUID
  "path": "/src/user_service.rs", // required
  "change_type": "edit"          // required: "edit", "create", "delete"
}
```

#### Network events

```json
// WebRequestMade
{
  "type": "web_request_made",
  "turn_id": "...",                          // optional, UUID
  "url": "https://api.github.com/repos/...", // required
  "method": "GET",                           // optional
  "status_code": 200                         // optional
}

// McpServerConnected
{
  "type": "mcp_server_connected",
  "session_id": "...",          // required, UUID
  "server_name": "filesystem"   // required
}
```

#### Cost and rate events

```json
// TokenUsageReported
{
  "type": "token_usage_reported",
  "turn_id": "...",            // optional, UUID
  "input_tokens": 1500,        // required
  "output_tokens": 300,        // required
  "cache_read_tokens": 100     // optional
}

// RateLimitHit
{
  "type": "rate_limit_hit",
  "session_id": "...",         // required, UUID
  "retry_after_secs": 60       // optional
}
```

#### Git events

```json
// GitCommitCreated
{ "type": "git_commit_created", "turn_id": "...", "sha": "abc123...", "message": "Add caching" }

// GitBranchCreated
{ "type": "git_branch_created", "turn_id": "...", "branch": "feature/cache" }

// PullRequestCreated
{ "type": "pull_request_created", "turn_id": "...", "identifier": "123", "title": "Add caching" }
```

#### Catch-all event

Use `Custom` for any application-specific event that doesn't fit the canonical types:

```json
{
  "type": "custom",
  "event_type": "hello_world",     // required, your custom event name
  "data": {                        // required, arbitrary JSON payload
    "message": "Hello from Telescope!",
    "tick": 42
  }
}
```

### Provenance values

Every collector declares provenance in its manifest. Valid values:

| `collector_type` | Description |
|-----------------|-------------|
| `mcp_proxy` | Live MCP traffic interception |
| `copilot_sdk` | Copilot SDK event hooks |
| `session_log` | Log file post-hoc parsing |
| `process_scan` | OS process enumeration |
| `os_kernel` | Kernel-level tracing (ETW, eBPF) |
| `self_report` | Agent self-reporting |
| `manual` | Manual user entry |
| `bridge` | Cross-device forwarding |

| `capture_method` | Description |
|------------------|-------------|
| `live_intercept` | Real-time traffic interception |
| `live_sdk_hook` | Real-time SDK callbacks |
| `live_kernel_event` | Real-time OS events |
| `post_hoc_log_parse` | Log parsing after the fact |
| `snapshot` | One-time process scan |
| `volunteered` | Agent self-reported |
| `inferred` | Heuristic inference |


## Next steps

- Browse `src/collectors/heartbeat/` for the simplest built-in collector.
- Browse `src/collectors/copilot-jsonl/` for a real-world file-based collector with incremental scanning.