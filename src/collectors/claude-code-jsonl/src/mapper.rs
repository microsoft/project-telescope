//! Mapper — transforms parsed Claude Code JSONL events into canonical `EventKind` JSON.
//!
//! Each JSONL event type is mapped to one or more canonical events. The output
//! is a JSON array of `EventKind` objects (serde-tagged with `"type"` and
//! `rename_all = "snake_case"`) suitable for the telescope processor.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use serde_json::json;
use uuid::Uuid;

use crate::parser::{self, ContentBlock, RawEvent};
use crate::scanner::SessionMeta;

/// Namespace UUID for deterministic ID generation (matches telescope core).
const TELESCOPE_NS: Uuid = Uuid::from_bytes([
    0x99, 0x23, 0xdf, 0xd5, 0x04, 0xe9, 0x5a, 0x68, 0xbd, 0xd9, 0xa6, 0x15, 0xce, 0xe9, 0x18, 0x6f,
]);

/// Track the last known model per session for `ModelSwitched` events.
static LAST_MODEL: std::sync::LazyLock<Mutex<HashMap<String, String>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Track turn indices per session for `TurnStarted` events.
static TURN_INDICES: std::sync::LazyLock<Mutex<HashMap<String, u32>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Sessions for which we have already emitted bootstrap events.
static BOOTSTRAPPED_SESSIONS: std::sync::LazyLock<Mutex<HashSet<Uuid>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashSet::new()));

/// Reset mapper state (called on collector start).
pub fn reset_state() {
    if let Ok(mut m) = LAST_MODEL.lock() {
        m.clear();
    }
    if let Ok(mut m) = TURN_INDICES.lock() {
        m.clear();
    }
    if let Ok(mut m) = BOOTSTRAPPED_SESSIONS.lock() {
        m.clear();
    }
}

/// Return `agent_discovered` + `session_started` events for a session the
/// first time it is encountered.
fn bootstrap_session_events(session_id: Uuid, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let already = BOOTSTRAPPED_SESSIONS
        .lock()
        .ok()
        .is_some_and(|mut set| !set.insert(session_id));
    if already {
        return Vec::new();
    }

    let agent_id = deterministic_uuid("claude-code");
    let version = meta.version.as_deref().unwrap_or("unknown");

    vec![
        json!({
            "type": "agent_discovered",
            "agent_id": agent_id.to_string(),
            "name": "Claude Code",
            "agent_type": "cli",
            "executable_path": null,
            "version": version,
        }),
        json!({
            "type": "session_started",
            "session_id": session_id.to_string(),
            "agent_id": agent_id.to_string(),
            "cwd": meta.cwd,
            "git_repo": null,
            "git_branch": meta.git_branch,
        }),
    ]
}

/// Map a single parsed JSONL event to canonical `EventKind` JSON objects.
pub fn map_event(event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let session_id = session_uuid(meta);
    let mut events = bootstrap_session_events(session_id, meta);

    let mut mapped = match event.event_type.as_str() {
        "user" => map_user(event, meta),
        "assistant" => map_assistant(event, meta),
        "system" => map_system(event, meta),
        "progress" => map_progress(event, meta),
        "file-history-snapshot" => vec![], // Internal bookkeeping, skip
        _ => vec![json!({
            "type": "custom",
            "event_type": event.event_type.clone(),
            "data": event.extra.clone(),
        })],
    };

    events.append(&mut mapped);
    events
}

// ── User events ──

fn map_user(event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let session_id = session_uuid(meta);

    // Check if this is a tool_result user message (contains tool results, not human input)
    if let Some(msg) = &event.message
        && let Some(content) = msg.get("content")
        && content.is_array()
    {
        let arr = content.as_array().unwrap();
        let has_tool_result = arr
            .iter()
            .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_result"));
        let has_text = arr
            .iter()
            .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"));

        if has_tool_result && !has_text {
            return map_tool_results(arr, event, meta);
        }
    }

    // Regular user message
    let content = parser::extract_user_content(event);
    let turn_id = event
        .uuid
        .as_deref()
        .and_then(parse_or_derive_uuid)
        .unwrap_or_else(Uuid::new_v4);

    vec![json!({
        "type": "user_message",
        "session_id": session_id.to_string(),
        "turn_id": turn_id.to_string(),
        "content": content,
    })]
}

/// Map `tool_result` content blocks to `tool_call_completed` events.
fn map_tool_results(
    blocks: &[serde_json::Value],
    event: &RawEvent,
    meta: &SessionMeta,
) -> Vec<serde_json::Value> {
    let mut events = Vec::new();

    for block in blocks {
        if block.get("type").and_then(|t| t.as_str()) != Some("tool_result") {
            continue;
        }

        let tool_use_id = block
            .get("tool_use_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let effect_id = parse_or_derive_uuid(tool_use_id).unwrap_or_else(Uuid::new_v4);

        // Check if the result indicates an error
        let is_error = block
            .get("is_error")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let status = if is_error { "failed" } else { "succeeded" };

        // Extract result content
        let result_content = block.get("content").cloned();

        events.push(json!({
            "type": "tool_call_completed",
            "effect_id": effect_id.to_string(),
            "status": status,
            "result": result_content,
            "duration_ms": null,
        }));
    }

    // If we produced no events, fall back to user_message
    if events.is_empty() {
        let session_id = session_uuid(meta);
        let turn_id = event
            .uuid
            .as_deref()
            .and_then(parse_or_derive_uuid)
            .unwrap_or_else(Uuid::new_v4);
        events.push(json!({
            "type": "user_message",
            "session_id": session_id.to_string(),
            "turn_id": turn_id.to_string(),
            "content": null,
        }));
    }

    events
}

// ── Assistant events ──

#[allow(clippy::too_many_lines)]
fn map_assistant(event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let session_id = session_uuid(meta);
    let Some(msg) = parser::extract_assistant_message(event) else {
        return vec![];
    };

    let model_name = msg.model.as_deref().unwrap_or("unknown");

    let mut events = Vec::new();

    // Track model changes
    if let Some(model_switch_event) = check_model_switch(&session_id.to_string(), model_name) {
        events.push(model_switch_event);
    }

    let turn_id = event
        .parent_uuid
        .as_deref()
        .and_then(parse_or_derive_uuid)
        .unwrap_or_else(|| {
            event
                .uuid
                .as_deref()
                .and_then(parse_or_derive_uuid)
                .unwrap_or_else(Uuid::new_v4)
        });

    // Increment turn index
    let turn_index = {
        let mut indices = TURN_INDICES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let idx = indices.entry(session_id.to_string()).or_insert(0);
        let current = *idx;
        *idx += 1;
        current
    };

    // Emit turn_started
    events.push(json!({
        "type": "turn_started",
        "session_id": session_id.to_string(),
        "turn_id": turn_id.to_string(),
        "turn_index": turn_index,
        "model_name": model_name,
    }));

    // Process content blocks
    let mut text_parts: Vec<String> = Vec::new();
    let mut tool_calls: Vec<serde_json::Value> = Vec::new();

    if let Some(content_blocks) = &msg.content {
        for block in content_blocks {
            match block {
                ContentBlock::Thinking { thinking, .. } => {
                    if let Some(text) = thinking
                        && !text.is_empty()
                    {
                        events.push(json!({
                            "type": "thinking_block",
                            "turn_id": turn_id.to_string(),
                            "content": text,
                        }));
                    }
                }
                ContentBlock::Text { text } => {
                    if let Some(t) = text {
                        text_parts.push(t.clone());
                    }
                }
                ContentBlock::ToolUse { id, name, input } => {
                    let tool_call_id = id
                        .as_deref()
                        .and_then(parse_or_derive_uuid)
                        .unwrap_or_else(Uuid::new_v4);
                    let tool_name = name.as_deref().unwrap_or("unknown");

                    let tool_event = json!({
                        "type": "tool_call_started",
                        "turn_id": turn_id.to_string(),
                        "effect_id": tool_call_id.to_string(),
                        "name": tool_name,
                        "arguments": input,
                    });
                    tool_calls.push(tool_event);

                    // Emit specialized events based on tool name
                    emit_specialized_tool_events(
                        &mut events,
                        tool_name,
                        input.as_ref(),
                        &turn_id,
                        &tool_call_id,
                    );
                }
                ContentBlock::ToolResult { .. } | ContentBlock::Unknown => {}
            }
        }
    }

    // Emit turn_completed with assistant text
    let assistant_text = if text_parts.is_empty() {
        None
    } else {
        Some(text_parts.join("\n"))
    };

    let tokens = msg.usage.as_ref().map(|u| {
        json!({
            "input_tokens": u.input_tokens,
            "output_tokens": u.output_tokens,
            "cache_creation_input_tokens": u.cache_creation_input_tokens,
            "cache_read_input_tokens": u.cache_read_input_tokens,
        })
    });

    events.push(json!({
        "type": "turn_completed",
        "session_id": session_id.to_string(),
        "turn_id": turn_id.to_string(),
        "user_message": null,
        "assistant_response": assistant_text,
        "model_name": model_name,
        "tokens": tokens,
        "duration_ms": null,
        "status": "completed",
    }));

    // Emit token usage
    if let Some(usage) = &msg.usage
        && let Some(input) = usage.input_tokens
    {
        events.push(json!({
            "type": "token_usage_reported",
            "turn_id": turn_id.to_string(),
            "input_tokens": input,
            "output_tokens": usage.output_tokens,
            "cache_read_tokens": usage.cache_read_input_tokens,
        }));
    }

    // Emit tool calls after the turn
    events.extend(tool_calls);

    events
}

/// Emit specialized canonical events based on tool name.
#[allow(clippy::too_many_lines)]
fn emit_specialized_tool_events(
    events: &mut Vec<serde_json::Value>,
    tool_name: &str,
    input: Option<&serde_json::Value>,
    turn_id: &Uuid,
    effect_id: &Uuid,
) {
    match tool_name {
        "Read" | "View" | "read" | "view" | "read_file" => {
            if let Some(path) = input
                .and_then(|a| a.get("file_path").or(a.get("path")))
                .and_then(|p| p.as_str())
            {
                events.push(json!({
                    "type": "file_read",
                    "turn_id": turn_id.to_string(),
                    "path": path,
                }));
            }
        }
        "Edit" | "edit" | "write" | "write_file" => {
            if let Some(path) = input
                .and_then(|a| a.get("file_path").or(a.get("path")))
                .and_then(|p| p.as_str())
            {
                events.push(json!({
                    "type": "file_written",
                    "turn_id": turn_id.to_string(),
                    "path": path,
                }));
            }
        }
        "Write" | "create" | "create_file" => {
            if let Some(path) = input
                .and_then(|a| a.get("file_path").or(a.get("path")))
                .and_then(|p| p.as_str())
            {
                events.push(json!({
                    "type": "file_created",
                    "turn_id": turn_id.to_string(),
                    "path": path,
                }));
            }
        }
        "Bash" | "bash" | "shell" | "terminal" => {
            let command = input
                .and_then(|a| a.get("command"))
                .and_then(|c| c.as_str())
                .unwrap_or("(unknown command)")
                .to_string();
            let cwd = input
                .and_then(|a| a.get("cwd"))
                .and_then(|c| c.as_str())
                .map(String::from);

            events.push(json!({
                "type": "shell_command_started",
                "turn_id": turn_id.to_string(),
                "effect_id": effect_id.to_string(),
                "command": command,
                "cwd": cwd,
            }));
        }
        "Grep" | "grep" | "search" | "find" => {
            let query = input
                .and_then(|a| a.get("pattern").or(a.get("query")))
                .and_then(|q| q.as_str())
                .unwrap_or("")
                .to_string();

            events.push(json!({
                "type": "search_performed",
                "turn_id": turn_id.to_string(),
                "query": query,
                "result_count": null,
            }));
        }
        "Glob" | "glob" => {
            let query = input
                .and_then(|a| a.get("pattern"))
                .and_then(|q| q.as_str())
                .unwrap_or("")
                .to_string();

            events.push(json!({
                "type": "search_performed",
                "turn_id": turn_id.to_string(),
                "query": query,
                "result_count": null,
            }));
        }
        "WebFetch" | "web_fetch" | "fetch" => {
            let url = input
                .and_then(|a| a.get("url"))
                .and_then(|u| u.as_str())
                .unwrap_or("")
                .to_string();

            events.push(json!({
                "type": "web_request_made",
                "turn_id": turn_id.to_string(),
                "url": url,
                "method": "GET",
                "status_code": null,
            }));
        }
        "Agent" => {
            let agent_type = input
                .and_then(|a| a.get("subagent_type").or(a.get("type")))
                .and_then(|t| t.as_str())
                .unwrap_or("general-purpose")
                .to_string();
            let prompt = input
                .and_then(|a| a.get("prompt"))
                .and_then(|p| p.as_str())
                .map(String::from);

            events.push(json!({
                "type": "sub_agent_spawned",
                "turn_id": turn_id.to_string(),
                "effect_id": effect_id.to_string(),
                "agent_type": agent_type,
                "prompt": prompt,
            }));
        }
        "Skill" => {
            let skill_name = input
                .and_then(|a| a.get("skill"))
                .and_then(|s| s.as_str())
                .unwrap_or("unknown")
                .to_string();

            events.push(json!({
                "type": "skill_invoked",
                "turn_id": turn_id.to_string(),
                "name": skill_name,
                "path": null,
            }));
        }
        "NotebookEdit" | "notebook_edit" => {
            if let Some(path) = input
                .and_then(|a| a.get("notebook_path").or(a.get("path")))
                .and_then(|p| p.as_str())
            {
                events.push(json!({
                    "type": "file_written",
                    "turn_id": turn_id.to_string(),
                    "path": path,
                }));
            }
        }
        _ => {}
    }
}

// ── System events ──

fn map_system(event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let session_id = session_uuid(meta);

    match event.subtype.as_deref() {
        Some("turn_duration") => {
            let mut events = Vec::new();

            // Emit session metadata with duration info
            if let Some(duration) = event.duration_ms {
                events.push(json!({
                    "type": "custom",
                    "event_type": "turn_duration",
                    "data": {
                        "session_id": session_id.to_string(),
                        "duration_ms": duration,
                        "slug": event.slug,
                    },
                }));
            }

            events
        }
        _ => vec![json!({
            "type": "custom",
            "event_type": format!("system.{}", event.subtype.as_deref().unwrap_or("unknown")),
            "data": {
                "session_id": session_id.to_string(),
            },
        })],
    }
}

// ── Progress events ──

fn map_progress(event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    // Progress events are streaming updates. We extract meaningful sub-events.
    let Some(data) = &event.data else {
        return vec![];
    };

    // Check for agent_progress (subagent activity)
    if let Some(msg_type) = data.get("type").and_then(|t| t.as_str())
        && msg_type == "agent_progress"
    {
        return map_agent_progress(data, event, meta);
    }

    // Other progress events are typically streaming chunks — skip for now
    vec![]
}

fn map_agent_progress(
    data: &serde_json::Value,
    _event: &RawEvent,
    meta: &SessionMeta,
) -> Vec<serde_json::Value> {
    let session_id = session_uuid(meta);

    // Agent progress contains subagent streaming data
    let agent_name = data
        .get("prompt")
        .and_then(|p| p.as_str())
        .unwrap_or("subagent");

    vec![json!({
        "type": "custom",
        "event_type": "agent_progress",
        "data": {
            "session_id": session_id.to_string(),
            "agent_name": agent_name,
        },
    })]
}

// ── Model tracking ──

/// Check for model switches and emit `model_switched` events.
fn check_model_switch(session_key: &str, new_model: &str) -> Option<serde_json::Value> {
    let mut last_model = LAST_MODEL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let previous = last_model.get(session_key).cloned();
    last_model.insert(session_key.to_string(), new_model.to_string());

    if let Some(prev) = previous
        && prev != new_model
    {
        return Some(json!({
            "type": "model_switched",
            "session_id": session_key,
            "from_model": prev,
            "to_model": new_model,
        }));
    }

    None
}

// ── Helpers ──

/// Derive a deterministic UUID v5 from a string key.
fn deterministic_uuid(key: &str) -> Uuid {
    Uuid::new_v5(&TELESCOPE_NS, key.as_bytes())
}

/// Parse a string as UUID, falling back to deterministic derivation.
fn parse_or_derive_uuid(s: &str) -> Option<Uuid> {
    s.parse::<Uuid>()
        .ok()
        .or_else(|| Some(deterministic_uuid(s)))
}

/// Get a session UUID from metadata.
fn session_uuid(meta: &SessionMeta) -> Uuid {
    parse_or_derive_uuid(&meta.session_id).unwrap_or_else(Uuid::new_v4)
}

/// Infer model provider from model name.
#[allow(dead_code)]
fn infer_provider(model_name: &str) -> Option<String> {
    let lower = model_name.to_lowercase();
    if lower.contains("claude")
        || lower.contains("haiku")
        || lower.contains("sonnet")
        || lower.contains("opus")
    {
        Some("anthropic".into())
    } else if lower.contains("gpt")
        || lower.contains("o1")
        || lower.contains("o3")
        || lower.contains("codex")
    {
        Some("openai".into())
    } else if lower.contains("gemini") {
        Some("google".into())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as TestMutex;

    static TEST_LOCK: TestMutex<()> = TestMutex::new(());

    fn lock_tests() -> std::sync::MutexGuard<'static, ()> {
        TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn default_meta() -> SessionMeta {
        SessionMeta {
            session_id: "test-session-123".into(),
            project_id: "C--code-test".into(),
            cwd: Some("C:\\project".into()),
            git_branch: Some("main".into()),
            version: Some("2.1.77".into()),
        }
    }

    #[test]
    fn test_bootstrap_emits_agent_and_session() {
        let _guard = lock_tests();
        reset_state();

        let meta = default_meta();
        let event = parser::RawEvent {
            event_type: "user".into(),
            uuid: Some("evt-1".into()),
            parent_uuid: None,
            timestamp: Some("2026-01-01T00:00:00Z".into()),
            is_sidechain: false,
            session_id: Some("test-session-123".into()),
            cwd: Some("C:\\project".into()),
            version: Some("2.1.77".into()),
            git_branch: Some("main".into()),
            user_type: Some("external".into()),
            permission_mode: None,
            message: Some(json!({"role": "user", "content": "hello"})),
            subtype: None,
            duration_ms: None,
            slug: None,
            data: None,
            message_id: None,
            snapshot: None,
            prompt_id: None,
            request_id: None,
            extra: serde_json::Map::new(),
        };

        let events = map_event(&event, &meta);
        let types: Vec<&str> = events
            .iter()
            .filter_map(|e| e.get("type").and_then(|t| t.as_str()))
            .collect();

        assert!(types.contains(&"agent_discovered"));
        assert!(types.contains(&"session_started"));
        assert!(types.contains(&"user_message"));

        // Second call should not re-emit bootstrap
        let events2 = map_event(&event, &meta);
        let types2: Vec<&str> = events2
            .iter()
            .filter_map(|e| e.get("type").and_then(|t| t.as_str()))
            .collect();

        assert!(!types2.contains(&"agent_discovered"));
        assert!(!types2.contains(&"session_started"));
    }

    #[test]
    fn test_map_user_message() {
        let _guard = lock_tests();
        reset_state();

        let meta = default_meta();
        let event = parser::RawEvent {
            event_type: "user".into(),
            uuid: Some("11111111-1111-1111-1111-111111111111".into()),
            parent_uuid: None,
            timestamp: Some("2026-01-01T00:00:00Z".into()),
            is_sidechain: false,
            session_id: Some("test-session-123".into()),
            cwd: Some("C:\\project".into()),
            version: Some("2.1.77".into()),
            git_branch: Some("main".into()),
            user_type: Some("external".into()),
            permission_mode: None,
            message: Some(json!({"role": "user", "content": "hello world"})),
            subtype: None,
            duration_ms: None,
            slug: None,
            data: None,
            message_id: None,
            snapshot: None,
            prompt_id: None,
            request_id: None,
            extra: serde_json::Map::new(),
        };

        let events = map_event(&event, &meta);

        let user_msg = events
            .iter()
            .find(|e| e.get("type").and_then(|t| t.as_str()) == Some("user_message"));
        assert!(user_msg.is_some());
        assert_eq!(
            user_msg.unwrap().get("content").and_then(|c| c.as_str()),
            Some("hello world")
        );
    }

    #[test]
    fn test_map_assistant_with_tool_use() {
        let _guard = lock_tests();
        reset_state();

        let meta = default_meta();
        let event = parser::RawEvent {
            event_type: "assistant".into(),
            uuid: Some("22222222-2222-2222-2222-222222222222".into()),
            parent_uuid: Some("11111111-1111-1111-1111-111111111111".into()),
            timestamp: Some("2026-01-01T00:00:01Z".into()),
            is_sidechain: false,
            session_id: Some("test-session-123".into()),
            cwd: None,
            version: Some("2.1.77".into()),
            git_branch: None,
            user_type: None,
            permission_mode: None,
            message: Some(json!({
                "model": "claude-opus-4-6",
                "id": "msg_01X",
                "type": "message",
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "let me check", "signature": "sig"},
                    {"type": "text", "text": "I'll read that file."},
                    {"type": "tool_use", "id": "toolu_01ABC", "name": "Read", "input": {"file_path": "/foo/bar.rs"}}
                ],
                "stop_reason": "tool_use",
                "usage": {"input_tokens": 100, "output_tokens": 50}
            })),
            subtype: None,
            duration_ms: None,
            slug: None,
            data: None,
            message_id: None,
            snapshot: None,
            prompt_id: None,
            request_id: None,
            extra: serde_json::Map::new(),
        };

        let events = map_event(&event, &meta);
        let types: Vec<&str> = events
            .iter()
            .filter_map(|e| e.get("type").and_then(|t| t.as_str()))
            .collect();

        assert!(types.contains(&"turn_started"));
        assert!(types.contains(&"thinking_block"));
        assert!(types.contains(&"turn_completed"));
        assert!(types.contains(&"token_usage_reported"));
        assert!(types.contains(&"tool_call_started"));
        assert!(types.contains(&"file_read"));
    }

    #[test]
    fn test_infer_provider() {
        assert_eq!(infer_provider("claude-opus-4-6"), Some("anthropic".into()));
        assert_eq!(infer_provider("gpt-4o"), Some("openai".into()));
        assert_eq!(infer_provider("gemini-2.0-flash"), Some("google".into()));
        assert_eq!(infer_provider("unknown-model"), None);
    }
}
