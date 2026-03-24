//! Parser — typed representation of Copilot JSONL events.
//!
//! Each line in `events.jsonl` is a JSON object with a universal schema:
//! `{ type, data, id, timestamp, parentId }`. This module provides typed
//! deserialization for all 28 known event types.

#![allow(dead_code)] // Fields parsed for completeness; not all are read yet.

use serde::Deserialize;

/// Top-level envelope shared by all JSONL events.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawEvent {
    /// Event type (e.g., "session.start", "user.message").
    #[serde(rename = "type")]
    pub event_type: String,
    /// Event-specific payload.
    pub data: serde_json::Value,
    /// Unique event ID (UUID v4).
    pub id: String,
    /// ISO 8601 timestamp.
    pub timestamp: String,
    /// Parent event ID for causality chain (null for root events).
    pub parent_id: Option<String>,
}

// ── Session lifecycle data ──

/// `session.start` payload.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStartData {
    pub copilot_version: Option<String>,
    pub producer: Option<String>,
    pub session_id: Option<String>,
    pub start_time: Option<String>,
}

/// `session.resume` payload.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResumeData {
    pub resume_time: Option<String>,
    pub event_count: Option<u64>,
    pub context: Option<SessionContext>,
    pub reasoning_effort: Option<String>,
}

/// Context object inside session.resume.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionContext {
    pub cwd: Option<String>,
    pub git_root: Option<String>,
    pub branch: Option<String>,
    pub head_commit: Option<String>,
    pub repository: Option<String>,
    pub host_type: Option<String>,
    pub base_commit: Option<String>,
}

/// `session.shutdown` payload.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionShutdownData {
    pub shutdown_type: Option<String>,
    pub total_premium_requests: Option<u64>,
    pub total_api_duration_ms: Option<u64>,
    pub session_start_time: Option<u64>,
    pub code_changes: Option<CodeChanges>,
    pub model_metrics: Option<serde_json::Value>,
    pub current_model: Option<String>,
}

/// Code change summary from shutdown.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeChanges {
    pub lines_added: Option<u64>,
    pub lines_removed: Option<u64>,
    pub files_modified: Option<Vec<String>>,
}

/// `session.context_changed` payload.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextChangedData {
    pub cwd: Option<String>,
    pub git_root: Option<String>,
    pub branch: Option<String>,
    pub head_commit: Option<String>,
    pub repository: Option<String>,
    pub host_type: Option<String>,
}

/// `session.mode_changed` payload.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeChangedData {
    pub previous_mode: Option<String>,
    pub new_mode: Option<String>,
}

/// `session.model_change` payload.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelChangeData {
    pub new_model: Option<String>,
}

/// `session.truncation` payload.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TruncationData {
    pub token_limit: Option<u64>,
    pub pre_truncation_tokens_in_messages: Option<u64>,
    pub post_truncation_tokens_in_messages: Option<u64>,
    pub tokens_removed_during_truncation: Option<u64>,
    pub messages_removed_during_truncation: Option<u64>,
    pub performed_by: Option<String>,
}

/// `session.compaction_complete` payload.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionCompleteData {
    pub success: Option<bool>,
    pub pre_compaction_tokens: Option<u64>,
    pub pre_compaction_messages_length: Option<u64>,
    pub summary_content: Option<String>,
    pub checkpoint_number: Option<u32>,
    pub checkpoint_path: Option<String>,
    pub compaction_tokens_used: Option<serde_json::Value>,
    pub request_id: Option<String>,
    pub error: Option<String>,
}

/// `session.warning` / `session.info` payload.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticData {
    pub warning_type: Option<String>,
    pub info_type: Option<String>,
    pub message: Option<String>,
}

/// `session.plan_changed` payload.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanChangedData {
    pub operation: Option<String>,
}

/// `session.task_complete` payload.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskCompleteData {
    pub summary: Option<String>,
    pub success: Option<bool>,
}

/// `session.workspace_file_changed` payload.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFileChangedData {
    pub path: Option<String>,
    pub operation: Option<String>,
}

// ── Conversation data ──

/// `user.message` payload.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserMessageData {
    pub content: Option<String>,
    pub transformed_content: Option<String>,
    pub attachments: Option<Vec<serde_json::Value>>,
    pub agent_mode: Option<String>,
    pub interaction_id: Option<String>,
    pub source: Option<String>,
}

/// `assistant.turn_start` payload.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnStartData {
    pub turn_id: Option<String>,
    pub interaction_id: Option<String>,
}

/// `assistant.message` payload.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantMessageData {
    pub message_id: Option<String>,
    pub content: Option<String>,
    pub tool_requests: Option<Vec<ToolRequest>>,
    pub output_tokens: Option<u64>,
    pub parent_tool_call_id: Option<String>,
    pub reasoning_opaque: Option<String>,
    pub reasoning_text: Option<String>,
    pub interaction_id: Option<String>,
}

/// A tool request from an assistant.message.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRequest {
    pub tool_call_id: Option<String>,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub call_type: Option<String>,
    pub arguments: Option<serde_json::Value>,
    pub intention_summary: Option<String>,
}

/// `assistant.turn_end` payload.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnEndData {
    pub turn_id: Option<String>,
}

// ── Tool execution data ──

/// `tool.execution_start` payload.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStartData {
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub arguments: Option<serde_json::Value>,
    pub mcp_server_name: Option<String>,
    pub mcp_tool_name: Option<String>,
    pub parent_tool_call_id: Option<String>,
}

/// `tool.execution_complete` payload.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCompleteData {
    pub tool_call_id: Option<String>,
    pub success: Option<bool>,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
    pub tool_telemetry: Option<serde_json::Value>,
    pub interaction_id: Option<String>,
    pub model: Option<String>,
    pub parent_tool_call_id: Option<String>,
}

// ── Sub-agent data ──

/// `subagent.started` payload.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentStartedData {
    pub tool_call_id: Option<String>,
    pub agent_name: Option<String>,
    pub agent_display_name: Option<String>,
    pub agent_description: Option<String>,
}

/// `subagent.completed` / `subagent.failed` payload.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentEndData {
    pub tool_call_id: Option<String>,
    pub agent_name: Option<String>,
    pub agent_display_name: Option<String>,
    pub error: Option<String>,
}

// ── Hook data ──

/// `hook.start` payload.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookStartData {
    pub hook_invocation_id: Option<String>,
    pub hook_type: Option<String>,
    pub input: Option<serde_json::Value>,
}

/// `hook.end` payload.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookEndData {
    pub hook_invocation_id: Option<String>,
    pub hook_type: Option<String>,
    pub success: Option<bool>,
}

// ── Skill data ──

/// `skill.invoked` payload.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillInvokedData {
    pub name: Option<String>,
    pub path: Option<String>,
    pub content: Option<String>,
}

// ── System data ──

/// `system.notification` payload.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemNotificationData {
    pub content: Option<String>,
    pub kind: Option<serde_json::Value>,
}

/// `abort` payload.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AbortData {
    pub reason: Option<String>,
}

/// Parse a raw JSONL line into a `RawEvent`.
pub fn parse_line(line: &str) -> Option<RawEvent> {
    serde_json::from_str(line).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_session_start() {
        let line = r#"{"type":"session.start","data":{"copilotVersion":"1.0.7","producer":"agency","sessionId":"abc-123","startTime":"2026-01-01T00:00:00Z","version":1},"id":"evt-1","timestamp":"2026-01-01T00:00:00Z","parentId":null}"#;
        let evt = parse_line(line).unwrap();
        assert_eq!(evt.event_type, "session.start");
        let data: SessionStartData = serde_json::from_value(evt.data).unwrap();
        assert_eq!(data.copilot_version.as_deref(), Some("1.0.7"));
        assert_eq!(data.session_id.as_deref(), Some("abc-123"));
    }

    #[test]
    fn test_parse_tool_execution() {
        let line = r#"{"type":"tool.execution_start","data":{"toolCallId":"tc-1","toolName":"grep","arguments":{"pattern":"foo"}},"id":"evt-2","timestamp":"2026-01-01T00:00:01Z","parentId":"evt-1"}"#;
        let evt = parse_line(line).unwrap();
        assert_eq!(evt.event_type, "tool.execution_start");
        let data: ToolStartData = serde_json::from_value(evt.data).unwrap();
        assert_eq!(data.tool_name.as_deref(), Some("grep"));
        assert_eq!(data.tool_call_id.as_deref(), Some("tc-1"));
    }

    #[test]
    fn test_parse_assistant_message() {
        let line = r#"{"type":"assistant.message","data":{"messageId":"msg-1","content":"Hello","toolRequests":[{"toolCallId":"tc-1","name":"view","type":"function","arguments":{"path":"foo.rs"}}],"outputTokens":42,"reasoningText":"thinking..."},"id":"evt-3","timestamp":"2026-01-01T00:00:02Z","parentId":"evt-2"}"#;
        let evt = parse_line(line).unwrap();
        let data: AssistantMessageData = serde_json::from_value(evt.data).unwrap();
        assert_eq!(data.content.as_deref(), Some("Hello"));
        assert_eq!(data.output_tokens, Some(42));
        assert_eq!(data.tool_requests.as_ref().unwrap().len(), 1);
        assert_eq!(
            data.tool_requests.as_ref().unwrap()[0].name.as_deref(),
            Some("view")
        );
    }
}
