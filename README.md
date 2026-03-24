# Project Telescope

**Local-first observability for AI agents. See what your agents are actually doing.**

AI agents are powerful — but opaque. They spawn sub-agents, call tools, read and write files, make network requests, and burn through tokens, all behind the scenes. Project Telescope gives you a window into all of it without sending a single byte off your machine.

This repo contains the **collector plugin system** — the open-source layer that captures telemetry from AI agents like GitHub Copilot, Claude, and any MCP-compatible agent. Write your own collectors for your agents and applications, or use the built-in ones. The Project Telescope service, dashboard, and CLI are available to be installed below.

---

## What is Project Telescope?

Project Telescope is a local-first observability tool for AI agents and MCPs, giving teams cross agent visibility into what agents did, why they did it, and where they got stuck. Think of it as `htop` for your AI agents: a live, structured view of everything happening beneath the surface.

It works with GitHub Copilot CLI, Claude, and any agent running on your machine. It requires no changes to your agents or your MCP servers. And **all data stays on your machine** — no cloud, no telemetry leaving your system, no third parties involved.

- **Faster debugging, audit trails, and iteration** — captures tool calls, conversation turns, reasoning and decisions, and friction signals in one place.
- **Background data collectors** — feed a set of local SQLite databases (`~/.telescope/*.db`) with no manual instrumentation required.
- **Privacy-first** — runs entirely on-device with no API keys or cloud dependency.
- **CLI** — `tele watch`, `tele sessions`, `tele insights`, `tele export`, and more, on Windows, macOS, and Linux.
- **Desktop dashboard  — an app for visual exploration of agent sessions, leaderboards, and execution graphs.

Think "DevTools for your AI pair programmer," purpose-built for local workflows. No config files, no API keys, no cloud accounts. Everything runs locally; everything stays on your machine. 

---

## Quickstart

### 1. Install Project Telescope

**Windows**

```
winget install telescope
```

Or download the MSI installer from the [releases page](https://github.com/microsoft/project-telescope/releases). This installs the background service, dashboard,  `tele` CLI, and all built-in collectors. 

**macOS**

```bash
curl -fsSL https://aka.ms/telescope/install.sh | bash
```

**Linux**

```bash
curl -fsSL https://aka.ms/telescope/install.sh | bash
```


All platforms install the full stack: the background service, the `tele` CLI, and all built-in collectors. Nothing leaves your machine.

### 2. Verify it's running

```bash
tele doctor
```

### 3. See what your agents are up to

```bash
tele watch        # Live stream of agent activity
tele sessions     # Browse recent sessions
tele insights     # Surface patterns and anomalies
```

### 4. (Optional) Build a custom collector

```bash
git clone https://github.com/microsoft/project-telescope.git
cd project-telescope/examples
cargo build --release
tele collector install ./target/release/
tele collector enable my-custom-collector
```

That's it. Your collector is now feeding events into the same pipeline as the built-in ones.

---

## How it works

Project Telescope has two layers: an open-source **collector** layer (this repo) and a **service** layer (installed via winget/MSI).

Collectors are shared libraries (`.dll` files) that watch AI agents and emit structured events. The service ingests those events, promotes them through a pipeline, and surfaces insights in the dashboard and CLI.

```
  Agent (GitHub Copilot, Claude, etc.)
              │
              │  stdin/stdout, JSONL files, SDK hooks
              ▼
  Collector (.dll plugin)        ← you are here (open-source)
              │
              │  Canonical events
              ▼
  Project Telescope Service  ← installed via winget
              │
        ┌─────┴─────┐
        ▼           ▼
   Dashboard       CLI
```

The key design principle: **every collector uses the exact same C-ABI interface**. There is no privileged native path. A collector you write has identical capabilities to a built-in one.

---

## Built-in collectors

These ship with Project Telescope and serve as reference implementations for building your own.

| Collector         | Type       | What it captures                                      |     |
| ----------------- | ---------- | ----------------------------------------------------- | --- |
| **MCP Proxy**     | Real-time  | JSON-RPC stdin/stdout interception from any MCP agent |     |
| **GitHub Copilot SDK**   | Real-time  | Hook events from GitHub Copilot CLI                          |     |
| **GitHub Copilot JSONL** | File-based | Scans `events.jsonl` session logs                     |     |
| **Claude JSONL**  | File-based | Imports Claude CLI exports                            |     |
| **Process Scan**  | One-shot   | Discovers AI agent OS processes                       |     |
| **Self-Report**   | Real-time  | MCP tool calls for agent self-reporting               |     |

---

## Writing your own collector

A collector is a `cdylib` shared library that exports a handful of C-ABI entry points. You can write collectors in Rust (using the SDK in `crates/cabi/`), or in any language that can produce a C-compatible shared library — C, C++, Go, Python via FFI, whatever you like.

### The C-ABI contract

```c
// Identity
const char* telescope_collector_name(void);
const char* telescope_collector_version(void);

// Lifecycle
int32_t telescope_collector_start(
    TelescopeCollectorHandle* wal_handle,
    const char* config_json
);
int32_t telescope_collector_stop(void);

// Data collection
int32_t telescope_collector_scan_once(...);
int32_t telescope_collector_process_event(
    const char* raw_event_json,
    char** canonical_events_json,
    void* user_data
);
```

### The collector manifest

Every collector ships with a `collector.toml` that describes what it is and how it should run:

```toml
[collector]
name = "my-custom-collector"
version = "0.1.0"
description = "Collects telemetry from <agent name>"
library = "telescope_collector_my_custom.dll"
collector_type = "custom"
capture_method = "log_import"

[config]
scan_interval_secs = 15
watch_dirs = ["~/.my-agent/"]
```

### Installing your collector

```bash
tele collector install ./path/to/my-collector/
tele collector enable my-custom-collector
tele collector list    # verify it's loaded
tele collector info my-custom-collector
```

---

## Canonical event types

Collectors emit events in a canonical format. The service understands ~40 event variants across these categories:

| Category | Examples |
|----------|---------|
| **Agent** | `AgentDiscovered`, `AgentHeartbeat` |
| **Session** | `SessionStarted`, `SessionEnded`, `SessionResumed` |
| **Turn** | `UserMessage`, `TurnStarted`, `TurnCompleted` |
| **Tool** | `ToolCallStarted`, `ToolCallCompleted` |
| **File** | `FileRead`, `FileWritten`, `FileCreated`, `FileDeleted` |
| **Shell** | `ShellCommandStarted`, `ShellCommandCompleted` |
| **Sub-Agent** | `SubAgentSpawned`, `SubAgentCompleted` |
| **Planning** | `PlanCreated`, `PlanStepCompleted`, `ThinkingBlock` |
| **Context** | `ContextWindowSnapshot`, `ContextPruned` |
| **Human-in-Loop** | `ApprovalRequested`, `ApprovalGranted`, `ApprovalDenied` |
| **Self-Report** | `IntentDeclared`, `DecisionMade`, `FrustrationReported` |
| **Model** | `ModelUsed`, `ModelSwitched` |
| **Error** | `ErrorOccurred`, `RetryAttempted` |
| **Code** | `SearchPerformed`, `CodeChangeApplied` |
| **Network** | `WebRequestMade`, `McpServerConnected` |
| **Cost** | `TokenUsageReported`, `RateLimitHit` |
| **Git** | `GitCommitCreated`, `GitBranchCreated`, `PullRequestCreated` |
| **Catch-all** | `Custom { event_type, data }` |

---

## Repo structure

```
microsoft/project-telescope/
├── src/
│   ├── crates/
│   │   ├── core/              # Core types, canonical events, WAL schema
│   │   └── cabi/              # C-ABI client SDK for building collectors
│   └── collectors/
│       ├── mcp_proxy/         # Built-in: MCP Proxy collector
│       ├── copilot_sdk/       # Built-in: GitHub Copilot SDK collector
│       ├── copilot_jsonl/     # Built-in: GitHub Copilot JSONL collector
│       ├── claude_jsonl/      # Built-in: Claude JSONL collector
│       ├── process_scan/      # Built-in: Process Scan collector
│       └── self_report/       # Built-in: Self-Report collector
├── examples/                  # Example third-party collector
|   └── collector.toml             # Manifest schema reference
├── docs/                      # Collector authoring guide
└── README.md
```

---

## Privacy and data

Project Telescope is local-first by design.

- **No network egress.** The service never phones home or transmits data externally.
- **All data is user-scoped** — SQLite databases live in your platform's user data directory (`%LOCALAPPDATA%` on Windows, `~/Library/Application Support` on macOS, `~/.local/share` on Linux).

---

## Contributing

We welcome contributions to the collector plugin system. Before submitting a pull request, you'll need to sign the [Microsoft Contributor License Agreement (CLA)](https://cla.opensource.microsoft.com/).

See [`docs/`](docs/) for the collector authoring guide and [`examples/`](examples/) for a starter template.

---

## License

The code in this repository is licensed under the [MIT License](LICENSE).

The Project Telescope service, dashboard, and CLI are distributed as closed-source binaries under Microsoft's standard software license terms, included with the installer.

