//! Claude Code JSONL collector — standalone binary using the Telescope SDK.
//!
//! Scans `~/.claude/projects/*/*.jsonl` for session events and maps them into
//! telescope canonical events. Supports all Claude Code event types (user,
//! assistant, system, progress) with incremental file reading (tail-follow pattern).

mod mapper;
mod parser;
mod scanner;

use std::time::Duration;

use telescope_collector_sdk::{Collector, CollectorManifest, EventKind, ProvenanceConfig};

struct ClaudeCodeJsonlCollector {
    /// Override for projects directory (for testing).
    projects_dir_override: Option<String>,
}

impl ClaudeCodeJsonlCollector {
    fn new() -> Self {
        Self {
            projects_dir_override: std::env::var("TELESCOPE_CLAUDE_PROJECTS_DIR").ok(),
        }
    }
}

#[async_trait::async_trait]
impl Collector for ClaudeCodeJsonlCollector {
    fn manifest(&self) -> CollectorManifest {
        CollectorManifest {
            name: "claude-code-jsonl".into(),
            version: "0.1.0".into(),
            description: "Imports Claude Code session data from JSONL conversation logs.".into(),
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
        if let Some(base) = scanner::projects_dir(self.projects_dir_override.as_deref())
            && base.is_dir()
        {
            scanner::snapshot_existing_positions(&base);
        }

        Ok(())
    }

    async fn collect(&mut self) -> anyhow::Result<Vec<EventKind>> {
        let base_dir = match scanner::projects_dir(self.projects_dir_override.as_deref()) {
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
    telescope_collector_sdk::run(ClaudeCodeJsonlCollector::new()).await
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
        let _guard = scanner::SCANNER_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let base = std::env::temp_dir().join("telescope_claude_code_jsonl_oop_test");
        let _ = fs::remove_dir_all(&base);
        let project_dir = base.join("C--code-test");
        fs::create_dir_all(&project_dir).unwrap();

        let mut collector = ClaudeCodeJsonlCollector {
            projects_dir_override: Some(base.to_string_lossy().to_string()),
        };

        // Start first (snapshots existing positions), then write data.
        collector.start().await.unwrap();

        let session_path = project_dir.join("test-session-001.jsonl");
        {
            let mut f = fs::File::create(&session_path).unwrap();
            writeln!(f, r#"{{"parentUuid":null,"isSidechain":false,"type":"user","message":{{"role":"user","content":"hello world"}},"uuid":"11111111-1111-1111-1111-111111111111","timestamp":"2026-01-01T00:00:00Z","cwd":"C:\\project","sessionId":"test-session-001","version":"2.1.77","gitBranch":"main","permissionMode":"default","userType":"external"}}"#).unwrap();
            writeln!(f, r#"{{"parentUuid":"11111111-1111-1111-1111-111111111111","isSidechain":false,"message":{{"model":"claude-opus-4-6","id":"msg_01X","type":"message","role":"assistant","content":[{{"type":"text","text":"Hello!"}}],"stop_reason":"end_turn","usage":{{"input_tokens":100,"output_tokens":10}}}},"type":"assistant","uuid":"22222222-2222-2222-2222-222222222222","timestamp":"2026-01-01T00:00:01Z"}}"#).unwrap();
            writeln!(f, r#"{{"parentUuid":"22222222-2222-2222-2222-222222222222","isSidechain":false,"message":{{"model":"claude-opus-4-6","id":"msg_02X","type":"message","role":"assistant","content":[{{"type":"tool_use","id":"toolu_01ABC","name":"Read","input":{{"file_path":"/foo/bar.rs"}}}}],"stop_reason":"tool_use","usage":{{"input_tokens":50,"output_tokens":20}}}},"type":"assistant","uuid":"33333333-3333-3333-3333-333333333333","timestamp":"2026-01-01T00:00:02Z"}}"#).unwrap();
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

        // Second collect should return empty (no new data).
        let events2 = collector.collect().await.unwrap();
        assert!(events2.is_empty(), "Second collect should return empty");

        let _ = fs::remove_dir_all(&base);
    }
}
