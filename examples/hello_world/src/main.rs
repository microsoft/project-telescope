// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//! Hello World collector — the simplest possible Telescope collector.
//!
//! Emits a `SessionStarted` event on first run and a `Custom` "hello_world"
//! event every 15 seconds. Use this as a starting point for your own collector.

use std::time::Duration;

use telescope_collector_sdk::{AgentConfig, Collector, CollectorManifest, EventKind};
use uuid::Uuid;

/// Deterministic agent ID so the same collector always maps to the same agent.
fn agent_id() -> Uuid {
    Uuid::new_v5(&Uuid::NAMESPACE_OID, b"telescope-hello-world-agent")
}

/// Deterministic session ID based on the agent and the current process.
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
        }
    }

    fn agent(&self) -> AgentConfig {
        AgentConfig {
            agent_id: "hello-world".into(),
            name: "Hello World Agent".into(),
            agent_type: "example".into(),
            version: Some("0.1.0".into()),
        }
    }

    async fn collect(&mut self) -> anyhow::Result<Vec<EventKind>> {
        self.tick += 1;
        let mut events = Vec::new();

        // On the first collect cycle, emit a session-started event.
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

        // Emit a custom hello world event every cycle.
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

    async fn stop(&mut self) -> anyhow::Result<()> {
        // Emit a session-ended event on shutdown.
        // In a real collector you would submit this via the SDK,
        // but for this example we just log it.
        eprintln!(
            "Hello World collector shutting down after {} ticks.",
            self.tick
        );
        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    telescope_collector_sdk::run(HelloWorldCollector::new()).await
}
