//! Parser — typed representation of Claude Code JSONL events.
//!
//! Each line in a Claude Code session JSONL is a JSON object with a `type`
//! field. Top-level event types: `user`, `assistant`, `progress`, `system`,
//! `file-history-snapshot`. This module provides typed deserialization for
//! all known event types.

#![allow(dead_code)] // Fields parsed for completeness; not all are read yet.

use serde::Deserialize;

/// Top-level envelope shared by all Claude Code JSONL events.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawEvent {
    /// Event type: "user", "assistant", "progress", "system", "file-history-snapshot".
    #[serde(rename = "type")]
    pub event_type: String,
    /// Unique event UUID.
    #[serde(default)]
    pub uuid: Option<String>,
    /// Parent event UUID for causality chain.
    #[serde(default)]
    pub parent_uuid: Option<String>,
    /// ISO 8601 timestamp.
    #[serde(default)]
    pub timestamp: Option<String>,
    /// Whether this is a sidechain (branched conversation).
    #[serde(default)]
    pub is_sidechain: bool,
    /// Session ID.
    #[serde(default)]
    pub session_id: Option<String>,
    /// Working directory.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Claude Code version.
    #[serde(default)]
    pub version: Option<String>,
    /// Git branch.
    #[serde(default)]
    pub git_branch: Option<String>,
    /// User type: "external" (human) or "internal" (agent).
    #[serde(default)]
    pub user_type: Option<String>,
    /// Permission mode.
    #[serde(default)]
    pub permission_mode: Option<String>,

    // ── User event fields ──
    /// Message content (present on user and assistant events).
    #[serde(default)]
    pub message: Option<serde_json::Value>,

    // ── System event fields ──
    /// System event subtype (e.g., "`turn_duration`").
    #[serde(default)]
    pub subtype: Option<String>,
    /// Duration in milliseconds (for `turn_duration` system events).
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// Session slug (for system events).
    #[serde(default)]
    pub slug: Option<String>,

    // ── Progress event fields ──
    /// Progress data wrapper.
    #[serde(default)]
    pub data: Option<serde_json::Value>,

    // ── File history snapshot fields ──
    /// Message ID for file snapshots.
    #[serde(default)]
    pub message_id: Option<String>,
    /// Snapshot data.
    #[serde(default)]
    pub snapshot: Option<serde_json::Value>,

    /// Prompt ID (present on some user events).
    #[serde(default)]
    pub prompt_id: Option<String>,

    /// Request ID (present on assistant events).
    #[serde(default)]
    pub request_id: Option<String>,

    /// All remaining fields we don't explicitly model.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ── Message content types ──

/// User message content.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserMessage {
    pub role: Option<String>,
    pub content: Option<serde_json::Value>,
}

/// Assistant message envelope (from the Claude API response).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AssistantMessage {
    pub model: Option<String>,
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub msg_type: Option<String>,
    pub role: Option<String>,
    pub content: Option<Vec<ContentBlock>>,
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
    pub usage: Option<Usage>,
}

/// A content block in an assistant message.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Extended thinking / reasoning.
    Thinking {
        thinking: Option<String>,
        #[serde(default)]
        signature: Option<String>,
    },
    /// Text output.
    Text { text: Option<String> },
    /// Tool use request.
    ToolUse {
        id: Option<String>,
        name: Option<String>,
        input: Option<serde_json::Value>,
    },
    /// Tool result (in user messages with tool results).
    ToolResult {
        tool_use_id: Option<String>,
        content: Option<serde_json::Value>,
    },
    /// Catch-all for unknown content block types.
    #[serde(other)]
    Unknown,
}

/// Token usage information from the API response.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub cache_creation: Option<CacheCreation>,
    pub service_tier: Option<String>,
}

/// Cache creation breakdown.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CacheCreation {
    pub ephemeral_5m_input_tokens: Option<u64>,
    pub ephemeral_1h_input_tokens: Option<u64>,
}

/// Parse a raw JSONL line into a `RawEvent`.
pub fn parse_line(line: &str) -> Option<RawEvent> {
    serde_json::from_str(line).ok()
}

/// Extract the user message content as a string from a user event.
pub fn extract_user_content(event: &RawEvent) -> Option<String> {
    let msg = event.message.as_ref()?;
    let user_msg: UserMessage = serde_json::from_value(msg.clone()).ok()?;

    match user_msg.content? {
        serde_json::Value::String(s) => Some(s),
        serde_json::Value::Array(arr) => {
            // Concatenate text blocks, skip tool_result blocks
            let parts: Vec<String> = arr
                .iter()
                .filter_map(|block| {
                    if block.get("type")?.as_str()? == "text" {
                        block.get("text")?.as_str().map(String::from)
                    } else {
                        None
                    }
                })
                .collect();
            if parts.is_empty() {
                None
            } else {
                Some(parts.join("\n"))
            }
        }
        _ => None,
    }
}

/// Extract the assistant message from an assistant event.
pub fn extract_assistant_message(event: &RawEvent) -> Option<AssistantMessage> {
    let msg = event.message.as_ref()?;
    serde_json::from_value(msg.clone()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_user_event() {
        let line = r#"{"parentUuid":null,"isSidechain":false,"type":"user","message":{"role":"user","content":"hello world"},"uuid":"abc-123","timestamp":"2026-01-01T00:00:00Z","cwd":"C:\\project","sessionId":"sess-1","version":"2.1.77","gitBranch":"main","permissionMode":"default","userType":"external"}"#;
        let evt = parse_line(line).unwrap();
        assert_eq!(evt.event_type, "user");
        assert_eq!(evt.uuid.as_deref(), Some("abc-123"));
        assert_eq!(evt.cwd.as_deref(), Some("C:\\project"));

        let content = extract_user_content(&evt);
        assert_eq!(content.as_deref(), Some("hello world"));
    }

    #[test]
    fn test_parse_assistant_event() {
        let line = r#"{"parentUuid":"abc-123","isSidechain":false,"message":{"model":"claude-opus-4-6","id":"msg_01X","type":"message","role":"assistant","content":[{"type":"text","text":"Hello!"}],"stop_reason":"end_turn","usage":{"input_tokens":100,"output_tokens":10}},"type":"assistant","uuid":"def-456","timestamp":"2026-01-01T00:00:01Z"}"#;
        let evt = parse_line(line).unwrap();
        assert_eq!(evt.event_type, "assistant");

        let msg = extract_assistant_message(&evt).unwrap();
        assert_eq!(msg.model.as_deref(), Some("claude-opus-4-6"));
        assert_eq!(msg.stop_reason.as_deref(), Some("end_turn"));
        assert!(msg.usage.is_some());
        assert_eq!(msg.usage.as_ref().unwrap().input_tokens, Some(100));
    }

    #[test]
    fn test_parse_assistant_with_tool_use() {
        let line = r#"{"parentUuid":"abc","isSidechain":false,"message":{"model":"claude-opus-4-6","id":"msg_02","type":"message","role":"assistant","content":[{"type":"thinking","thinking":"let me think","signature":"sig"},{"type":"tool_use","id":"toolu_01","name":"Read","input":{"file_path":"/foo/bar.rs"}}],"stop_reason":"tool_use","usage":{"input_tokens":50,"output_tokens":20}},"type":"assistant","uuid":"ghi-789","timestamp":"2026-01-01T00:00:02Z"}"#;
        let evt = parse_line(line).unwrap();
        let msg = extract_assistant_message(&evt).unwrap();

        let content = msg.content.unwrap();
        assert_eq!(content.len(), 2);

        match &content[0] {
            ContentBlock::Thinking { thinking, .. } => {
                assert_eq!(thinking.as_deref(), Some("let me think"));
            }
            _ => panic!("Expected Thinking block"),
        }

        match &content[1] {
            ContentBlock::ToolUse { id, name, .. } => {
                assert_eq!(id.as_deref(), Some("toolu_01"));
                assert_eq!(name.as_deref(), Some("Read"));
            }
            _ => panic!("Expected ToolUse block"),
        }
    }

    #[test]
    fn test_parse_system_event() {
        let line = r#"{"parentUuid":"abc","isSidechain":false,"type":"system","subtype":"turn_duration","durationMs":5000,"timestamp":"2026-01-01T00:00:03Z","uuid":"sys-1","sessionId":"sess-1","version":"2.1.77","gitBranch":"main"}"#;
        let evt = parse_line(line).unwrap();
        assert_eq!(evt.event_type, "system");
        assert_eq!(evt.subtype.as_deref(), Some("turn_duration"));
        assert_eq!(evt.duration_ms, Some(5000));
    }
}
