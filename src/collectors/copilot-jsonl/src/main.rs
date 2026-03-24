//! Copilot JSONL collector — standalone binary using the Telescope SDK.
//!
//! Scans `~/.copilot/session-state/*/events.jsonl` for session events and maps
//! them into telescope canonical events. Supports all 28 Copilot JSONL event
//! types with incremental file reading (tail-follow pattern).

mod mapper;
mod parser;
mod scanner;

use std::time::Duration;

use telescope_collector_sdk::{Collector, CollectorManifest, EventKind, ProvenanceConfig};

struct CopilotJsonlCollector {
    /// Override for session state directory (for testing).
    session_dir_override: Option<String>,
}

impl CopilotJsonlCollector {
    fn new() -> Self {
        Self {
            session_dir_override: std::env::var("TELESCOPE_COPILOT_SESSION_DIR").ok(),
        }
    }
}

#[async_trait::async_trait]
impl Collector for CopilotJsonlCollector {
    fn manifest(&self) -> CollectorManifest {
        CollectorManifest {
            name: "copilot-jsonl".into(),
            version: "0.1.0".into(),
            description: "Imports Copilot CLI session data from JSONL event logs.".into(),
            provenance: ProvenanceConfig {
                collector_type: "session_log".into(),
                capture_method: "post_hoc_log_parse".into(),
            },
        }
    }

    async fn start(&mut self) -> anyhow::Result<()> {
        scanner::reset_positions();
        mapper::reset_state();

        // Snapshot existing file sizes so the first collect() only picks up
        // data appended after this point.
        if let Some(base) = scanner::session_state_dir(self.session_dir_override.as_deref())
            && base.is_dir()
        {
            scanner::snapshot_existing_positions(&base);
        }

        Ok(())
    }

    async fn collect(&mut self) -> anyhow::Result<Vec<EventKind>> {
        let base_dir = match scanner::session_state_dir(self.session_dir_override.as_deref()) {
            Some(d) if d.is_dir() => d,
            _ => return Ok(vec![]),
        };

        let Some(batches) = scanner::collect_new_events(&base_dir) else {
            return Ok(vec![]);
        };

        // Combine scanner + mapper (previously split across collect/process C-ABI calls).
        let mut all_events = Vec::new();
        for batch in &batches {
            for line in &batch.lines {
                let Some(raw_event) = parser::parse_line(line) else {
                    continue;
                };
                let canonical_json = mapper::map_event(&raw_event, &batch.meta);
                for val in canonical_json {
                    if let Ok(event) = serde_json::from_value::<EventKind>(val) {
                        all_events.push(event);
                    }
                }
            }
        }

        Ok(all_events)
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(30)
    }

    async fn stop(&mut self) -> anyhow::Result<()> {
        scanner::reset_positions();
        mapper::reset_state();
        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    telescope_collector_sdk::run(CopilotJsonlCollector::new()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    /// End-to-end test: create temp JSONL files, collect, verify output.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_collect_roundtrip() {
        // Serialize with scanner tests to avoid `FILE_POSITIONS` races.
        let _guard = scanner::SCANNER_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let base = std::env::temp_dir().join("telescope_copilot_jsonl_oop_test");
        let _ = fs::remove_dir_all(&base);
        let session_dir = base.join("test-session-001");
        fs::create_dir_all(&session_dir).unwrap();

        fs::write(
            session_dir.join("workspace.yaml"),
            "id: test-session-001\ncwd: D:\\project\nbranch: main\nsummary: Test\n",
        )
        .unwrap();

        let mut collector = CopilotJsonlCollector {
            session_dir_override: Some(base.to_string_lossy().to_string()),
        };

        // Start first (snapshots existing positions), then write data.
        collector.start().await.unwrap();

        {
            let mut f = fs::File::create(session_dir.join("events.jsonl")).unwrap();
            writeln!(f, r#"{{"type":"session.start","data":{{"copilotVersion":"1.0.7","producer":"agency","sessionId":"test-session-001","startTime":"2026-01-01T00:00:00Z","version":1}},"id":"evt-1","timestamp":"2026-01-01T00:00:00Z","parentId":null}}"#).unwrap();
            writeln!(f, r#"{{"type":"user.message","data":{{"content":"Hello","interactionId":"11111111-1111-1111-1111-111111111111"}},"id":"evt-2","timestamp":"2026-01-01T00:00:01Z","parentId":"evt-1"}}"#).unwrap();
            writeln!(f, r#"{{"type":"tool.execution_start","data":{{"toolCallId":"22222222-2222-2222-2222-222222222222","toolName":"view","arguments":{{"path":"foo.rs"}}}},"id":"evt-3","timestamp":"2026-01-01T00:00:02Z","parentId":"evt-2"}}"#).unwrap();
        }

        let events = collector.collect().await.unwrap();

        assert!(!events.is_empty(), "Should produce canonical events");

        let type_names: Vec<&str> = events.iter().map(EventKind::type_tag).collect();

        assert!(
            type_names.contains(&"agent_discovered"),
            "Should have agent_discovered, got: {type_names:?}"
        );
        assert!(
            type_names.contains(&"session_started"),
            "Should have session_started"
        );
        assert!(
            type_names.contains(&"user_message"),
            "Should have user_message"
        );
        assert!(
            type_names.contains(&"tool_call_started"),
            "Should have tool_call_started"
        );

        // Second collect should return empty (no new data).
        let events2 = collector.collect().await.unwrap();
        assert!(events2.is_empty(), "Second collect should return empty");

        let _ = fs::remove_dir_all(&base);
    }
}
