// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//! Heartbeat test collector — standalone binary using the Telescope SDK.
//!
//! Emits periodic `AgentHeartbeat` + `Custom` events as a proof of concept
//! for the out-of-process collector architecture.

use std::time::Duration;

use telescope_collector_sdk::{AgentConfig, Collector, CollectorManifest, EventKind};
use uuid::Uuid;

/// Deterministic agent UUID derived from the collector name.
fn agent_id() -> Uuid {
    Uuid::new_v5(&Uuid::NAMESPACE_OID, b"telescope-heartbeat-test-agent")
}

struct HeartbeatCollector {
    counter: u64,
    agent_id: Uuid,
}

impl HeartbeatCollector {
    fn new() -> Self {
        Self {
            counter: 0,
            agent_id: agent_id(),
        }
    }
}

#[async_trait::async_trait]
impl Collector for HeartbeatCollector {
    fn manifest(&self) -> CollectorManifest {
        CollectorManifest {
            name: "heartbeat".into(),
            version: "0.1.0".into(),
            description: "Simple test collector that emits periodic heartbeat events.".into(),
        }
    }

    fn agent(&self) -> AgentConfig {
        AgentConfig {
            agent_id: "telescope-heartbeat".into(),
            name: "Telescope Heartbeat".into(),
            agent_type: "test".into(),
            version: None,
        }
    }

    async fn collect(&mut self) -> anyhow::Result<Vec<EventKind>> {
        self.counter += 1;
        Ok(vec![
            EventKind::AgentHeartbeat {
                agent_id: self.agent_id,
            },
            EventKind::Custom {
                event_type: "heartbeat_tick".into(),
                data: serde_json::json!({
                    "seq": self.counter,
                    "collector": "heartbeat",
                }),
            },
        ])
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(10)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    telescope_collector_sdk::run(HeartbeatCollector::new()).await
}
