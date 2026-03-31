// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//! Mapper — transforms parsed Copilot JSONL events into canonical `EventKind` JSON.
//!
//! Each JSONL event type is mapped to one or more canonical events. The output
//! is a JSON array of `EventKind` objects (serde-tagged with `"type"` and
//! `rename_all = "snake_case"`) suitable for the telescope processor.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use serde_json::json;
use uuid::Uuid;

use crate::parser::{self, RawEvent};
use crate::scanner::SessionMeta;

/// Namespace UUID for deterministic ID generation (matches telescope core).
const TELESCOPE_NS: Uuid = Uuid::from_bytes([
    0x99, 0x23, 0xdf, 0xd5, 0x04, 0xe9, 0x5a, 0x68, 0xbd, 0xd9, 0xa6, 0x15, 0xce, 0xe9, 0x18, 0x6f,
]);

/// Track the last known model per session so we can populate `from_model`
/// on `ModelSwitched` events.
static LAST_MODEL: std::sync::LazyLock<Mutex<HashMap<String, String>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Track turn indices per session for `TurnStarted` events.
static TURN_INDICES: std::sync::LazyLock<Mutex<HashMap<String, u32>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Sessions for which we have already emitted bootstrap events
/// (`agent_discovered` + `session_started`).  Prevents duplicates within a
/// single collector lifecycle while ensuring that the very first event we
/// see for any session — regardless of type — establishes the correct
/// agent-session binding.
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
/// first time it is encountered.  Subsequent calls for the same session
/// return an empty vec.  This guarantees the processor always has the
/// correct agent-session binding, even when `session.start` was never
/// observed (e.g. historical data was skipped on startup).
fn bootstrap_session_events(session_id: Uuid, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let already = BOOTSTRAPPED_SESSIONS
        .lock()
        .ok()
        .is_some_and(|mut set| !set.insert(session_id));
    if already {
        return Vec::new();
    }

    let agent_id = deterministic_uuid("copilot-cli");
    vec![
        json!({
            "type": "agent_discovered",
            "agent_id": agent_id.to_string(),
            "name": "Copilot CLI",
            "agent_type": "cli",
            "executable_path": null,
            "version": "unknown",
        }),
        json!({
            "type": "session_started",
            "session_id": session_id.to_string(),
            "agent_id": agent_id.to_string(),
            "cwd": meta.cwd,
            "git_repo": null,
            "git_branch": meta.branch,
        }),
    ]
}

/// Map a single parsed JSONL event to canonical `EventKind` JSON objects.
///
/// Returns a `Vec` of `serde_json` values, each representing one canonical event.
/// The caller wraps these into a JSON array for `process()` output.
///
/// On the first event seen for any session, bootstrap events
/// (`agent_discovered` + `session_started`) are automatically prepended so
/// the processor always has the correct agent → session binding — even when
/// `session.start` was never observed.
pub fn map_event(event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    // Ensure agent + session context exists before any session-scoped event.
    let session_id = session_uuid(meta);
    let mut events = bootstrap_session_events(session_id, meta);

    let mut mapped = match event.event_type.as_str() {
        "session.start" => map_session_start(event, meta),
        "session.resume" => map_session_resume(event, meta),
        "session.shutdown" => map_session_shutdown(event, meta),
        "session.context_changed" => map_context_changed(event, meta),
        "session.mode_changed" => map_mode_changed(event, meta),
        "session.model_change" => map_model_change(event, meta),
        "session.truncation" => map_truncation(event, meta),
        "session.compaction_start" => map_compaction_start(event, meta),
        "session.compaction_complete" => map_compaction_complete(event, meta),
        "session.warning" => map_warning(event, meta),
        "session.info" => map_info(event, meta),
        "session.plan_changed" => map_plan_changed(event, meta),
        "session.task_complete" => map_task_complete(event, meta),
        "session.workspace_file_changed" => map_workspace_file_changed(event, meta),
        "user.message" => map_user_message(event, meta),
        "assistant.turn_start" => map_turn_start(event, meta),
        "assistant.message" => map_assistant_message(event, meta),
        "assistant.turn_end" => vec![], // Covered by assistant.message
        "tool.execution_start" => map_tool_start(event, meta),
        "tool.execution_complete" => map_tool_complete(event),
        "subagent.started" => map_subagent_started(event, meta),
        "subagent.completed" => map_subagent_completed(event),
        "subagent.failed" => map_subagent_failed(event),
        "hook.start" => map_hook_start(event, meta),
        "hook.end" => map_hook_end(event),
        "skill.invoked" => map_skill_invoked(event, meta),
        "system.notification" => map_system_notification(event, meta),
        "abort" => map_abort(event, meta),
        _ => vec![json!({
            "type": "custom",
            "event_type": event.event_type.clone(),
            "data": event.data.clone(),
        })],
    };

    events.append(&mut mapped);
    events
}

// ── Session lifecycle ──

fn map_session_start(event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let data: parser::SessionStartData =
        serde_json::from_value(event.data.clone()).unwrap_or_default();

    let session_id = data
        .session_id
        .as_deref()
        .or(Some(&meta.dir_id))
        .and_then(parse_or_derive_uuid)
        .unwrap_or_else(Uuid::new_v4);

    let version = data.copilot_version.as_deref().unwrap_or("unknown");
    let agent_id = deterministic_uuid("copilot-cli");

    // Track model from producer if available
    if let Some(model) = data.copilot_version.as_ref()
        && let Ok(mut m) = LAST_MODEL.lock()
    {
        m.insert(session_id.to_string(), model.clone());
    }

    // Re-emit agent_discovered with the real version from session.start
    // (the bootstrap in map_event uses "unknown"; this overwrites it via upsert).
    let mut events = vec![json!({
        "type": "agent_discovered",
        "agent_id": agent_id.to_string(),
        "name": "Copilot CLI",
        "agent_type": "cli",
        "executable_path": null,
        "version": version,
    })];

    // Emit session metadata with extra context from workspace.yaml
    if meta.summary.is_some() || meta.git_root.is_some() {
        events.push(json!({
            "type": "session_metadata_updated",
            "session_id": session_id.to_string(),
            "metadata": {
                "summary": meta.summary,
                "git_root": meta.git_root,
                "producer": data.producer,
                "dir_id": meta.dir_id,
            },
        }));
    }

    events
}

fn map_session_resume(event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let data: parser::SessionResumeData =
        serde_json::from_value(event.data.clone()).unwrap_or_default();

    let session_id = session_uuid(meta);

    let mut events = vec![json!({
        "type": "session_resumed",
        "session_id": session_id.to_string(),
    })];

    // Extract context updates
    if let Some(ctx) = &data.context {
        events.push(json!({
            "type": "session_metadata_updated",
            "session_id": session_id.to_string(),
            "metadata": {
                "cwd": ctx.cwd,
                "git_root": ctx.git_root,
                "branch": ctx.branch,
                "head_commit": ctx.head_commit,
                "repository": ctx.repository,
                "host_type": ctx.host_type,
                "reasoning_effort": data.reasoning_effort,
            },
        }));
    }

    events
}

fn map_session_shutdown(event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let data: parser::SessionShutdownData =
        serde_json::from_value(event.data.clone()).unwrap_or_default();

    let session_id = session_uuid(meta);
    let status = match data.shutdown_type.as_deref() {
        Some("error") => "errored",
        Some("user_initiated") => "cancelled",
        _ => "completed",
    };

    // Calculate duration from session_start_time if available
    let duration_ms = data.session_start_time.and_then(|start| {
        let now_ms = chrono::Utc::now().timestamp_millis().cast_unsigned();
        if now_ms > start {
            u32::try_from(now_ms - start).ok()
        } else {
            None
        }
    });

    let mut events = vec![json!({
        "type": "session_ended",
        "session_id": session_id.to_string(),
        "status": status,
        "duration_ms": duration_ms,
    })];

    // Store code changes and premium requests in session metadata
    if data.code_changes.is_some() || data.total_premium_requests.is_some() {
        events.push(json!({
            "type": "session_metadata_updated",
            "session_id": session_id.to_string(),
            "metadata": {
                "code_changes": data.code_changes.as_ref().map(|cc| json!({
                    "lines_added": cc.lines_added,
                    "lines_removed": cc.lines_removed,
                    "files_modified": cc.files_modified,
                })),
                "total_premium_requests": data.total_premium_requests,
                "total_api_duration_ms": data.total_api_duration_ms,
            },
        }));
    }

    // Emit ModelUsed for each model in modelMetrics
    if let Some(metrics) = &data.model_metrics
        && let Some(obj) = metrics.as_object()
    {
        for (model_name, model_data) in obj {
            let usage = model_data.get("usage").cloned().unwrap_or(json!({}));
            let request_cost = model_data
                .get("requests")
                .and_then(|r| r.get("cost"))
                .and_then(serde_json::Value::as_f64);
            let request_count = model_data
                .get("requests")
                .and_then(|r| r.get("count"))
                .and_then(serde_json::Value::as_u64)
                .and_then(|c| u32::try_from(c).ok());

            events.push(json!({
                "type": "model_used",
                "session_id": session_id.to_string(),
                "name": model_name,
                "provider": infer_provider(model_name),
                "tokens": {
                    "input_tokens": usage.get("inputTokens"),
                    "output_tokens": usage.get("outputTokens"),
                    "cache_read_tokens": usage.get("cacheReadTokens"),
                    "cache_write_tokens": usage.get("cacheWriteTokens"),
                },
                "cost": {
                    "total_cost_usd": request_cost,
                },
                "invocation_count": request_count,
            }));

            // Also emit token usage
            if let Some(input) = usage.get("inputTokens").and_then(serde_json::Value::as_u64) {
                events.push(json!({
                        "type": "token_usage_reported",
                        "turn_id": Uuid::new_v4().to_string(),
                        "input_tokens": input,
                        "output_tokens": usage.get("outputTokens").and_then(serde_json::Value::as_u64),
                        "cache_read_tokens": usage.get("cacheReadTokens").and_then(serde_json::Value::as_u64),
                    }));
            }

            // Track request count in custom event
            if let Some(count) = request_count {
                events.push(json!({
                    "type": "custom",
                    "event_type": "model_request_count",
                    "data": {
                        "model": model_name,
                        "count": count,
                        "session_id": session_id.to_string(),
                    },
                }));
            }
        }
    }

    events
}

fn map_context_changed(event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let data: parser::ContextChangedData =
        serde_json::from_value(event.data.clone()).unwrap_or_default();

    let session_id = session_uuid(meta);

    vec![json!({
        "type": "session_metadata_updated",
        "session_id": session_id.to_string(),
        "metadata": {
            "cwd": data.cwd,
            "git_root": data.git_root,
            "branch": data.branch,
            "head_commit": data.head_commit,
            "repository": data.repository,
            "host_type": data.host_type,
        },
    })]
}

fn map_mode_changed(event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let data: parser::ModeChangedData =
        serde_json::from_value(event.data.clone()).unwrap_or_default();

    let session_id = session_uuid(meta);

    vec![json!({
        "type": "session_mode_changed",
        "session_id": session_id.to_string(),
        "previous_mode": data.previous_mode.unwrap_or("unknown".into()),
        "new_mode": data.new_mode.unwrap_or("unknown".into()),
    })]
}

fn map_model_change(event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let data: parser::ModelChangeData =
        serde_json::from_value(event.data.clone()).unwrap_or_default();

    let session_id = session_uuid(meta);
    let new_model = data.new_model.unwrap_or("unknown".into());

    let from_model = LAST_MODEL
        .lock()
        .ok()
        .and_then(|map| map.get(&session_id.to_string()).cloned())
        .unwrap_or("unknown".into());

    // Update tracked model
    if let Ok(mut m) = LAST_MODEL.lock() {
        m.insert(session_id.to_string(), new_model.clone());
    }

    vec![json!({
        "type": "model_switched",
        "session_id": session_id.to_string(),
        "from_model": from_model,
        "to_model": new_model,
    })]
}

fn map_truncation(event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let data: parser::TruncationData =
        serde_json::from_value(event.data.clone()).unwrap_or_default();

    let session_id = session_uuid(meta);
    let mut events = Vec::new();

    if let Some(removed) = data.tokens_removed_during_truncation {
        events.push(json!({
            "type": "context_pruned",
            "session_id": session_id.to_string(),
            "tokens_removed": removed,
        }));
    }

    if let Some(total) = data.post_truncation_tokens_in_messages {
        events.push(json!({
            "type": "context_window_snapshot",
            "session_id": session_id.to_string(),
            "total_tokens": total,
            "max_tokens": data.token_limit,
        }));
    }

    if events.is_empty() {
        // Still emit something so we don't lose the event
        events.push(json!({
            "type": "context_pruned",
            "session_id": session_id.to_string(),
            "tokens_removed": 0,
        }));
    }

    events
}

fn map_compaction_start(_event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let session_id = session_uuid(meta);
    vec![json!({
        "type": "compaction_started",
        "session_id": session_id.to_string(),
    })]
}

fn map_compaction_complete(event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let data: parser::CompactionCompleteData =
        serde_json::from_value(event.data.clone()).unwrap_or_default();

    let session_id = session_uuid(meta);

    vec![json!({
        "type": "compaction_completed",
        "session_id": session_id.to_string(),
        "success": data.success.unwrap_or(false),
        "pre_compaction_tokens": data.pre_compaction_tokens,
        "checkpoint_number": data.checkpoint_number,
        "compaction_tokens_used": data.compaction_tokens_used,
    })]
}

fn map_warning(event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let data: parser::DiagnosticData =
        serde_json::from_value(event.data.clone()).unwrap_or_default();

    let session_id = session_uuid(meta);

    vec![json!({
        "type": "error_occurred",
        "turn_id": null,
        "session_id": session_id.to_string(),
        "message": data.message.unwrap_or_default(),
        "category": data.warning_type.unwrap_or("warning".into()),
    })]
}

fn map_info(event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let data: parser::DiagnosticData =
        serde_json::from_value(event.data.clone()).unwrap_or_default();

    let session_id = session_uuid(meta);

    vec![json!({
        "type": "custom",
        "event_type": "session_info",
        "data": {
            "info_type": data.info_type,
            "message": data.message,
            "session_id": session_id.to_string(),
        },
    })]
}

fn map_plan_changed(event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let data: parser::PlanChangedData =
        serde_json::from_value(event.data.clone()).unwrap_or_default();

    let session_id = session_uuid(meta);
    let turn_id = turn_uuid_from_event(event, meta);
    let operation = data.operation.unwrap_or("unknown".into());

    if operation == "create" {
        vec![json!({
            "type": "plan_created",
            "turn_id": turn_id.to_string(),
            "content": format!("Plan {operation}"),
        })]
    } else {
        vec![json!({
            "type": "custom",
            "event_type": "plan_changed",
            "data": {
                "operation": operation,
                "session_id": session_id.to_string(),
            },
        })]
    }
}

fn map_task_complete(event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let data: parser::TaskCompleteData =
        serde_json::from_value(event.data.clone()).unwrap_or_default();

    let turn_id = turn_uuid_from_event(event, meta);

    vec![json!({
        "type": "outcome_reported",
        "turn_id": turn_id.to_string(),
        "outcome": data.summary.unwrap_or("Task completed".into()),
        "success": data.success.unwrap_or(true),
    })]
}

fn map_workspace_file_changed(event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let data: parser::WorkspaceFileChangedData =
        serde_json::from_value(event.data.clone()).unwrap_or_default();

    let turn_id = turn_uuid_from_event(event, meta);
    let path = data.path.unwrap_or_default();
    let operation = data.operation.as_deref().unwrap_or("modify");

    let event_type = match operation {
        "create" => "file_created",
        "delete" => "file_deleted",
        _ => "file_written",
    };

    vec![json!({
        "type": event_type,
        "turn_id": turn_id.to_string(),
        "path": path,
    })]
}

// ── Conversation events ──

fn map_user_message(event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let data: parser::UserMessageData =
        serde_json::from_value(event.data.clone()).unwrap_or_default();

    let session_id = session_uuid(meta);
    let turn_id = data
        .interaction_id
        .as_deref()
        .and_then(|s| s.parse::<Uuid>().ok())
        .unwrap_or_else(|| deterministic_uuid(&event.id));

    vec![json!({
        "type": "user_message",
        "session_id": session_id.to_string(),
        "turn_id": turn_id.to_string(),
        "content": data.content,
    })]
}

fn map_turn_start(event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let data: parser::TurnStartData =
        serde_json::from_value(event.data.clone()).unwrap_or_default();

    let session_id = session_uuid(meta);
    let turn_id = data
        .interaction_id
        .as_deref()
        .and_then(|s| s.parse::<Uuid>().ok())
        .unwrap_or_else(|| deterministic_uuid(&event.id));

    // Increment turn index for this session
    let turn_index = {
        let mut indices = TURN_INDICES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let idx = indices.entry(session_id.to_string()).or_insert(0);
        let current = *idx;
        *idx += 1;
        current
    };

    vec![json!({
        "type": "turn_started",
        "session_id": session_id.to_string(),
        "turn_id": turn_id.to_string(),
        "turn_index": turn_index,
        "model_name": null,
    })]
}

fn map_assistant_message(event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let data: parser::AssistantMessageData =
        serde_json::from_value(event.data.clone()).unwrap_or_default();

    let session_id = session_uuid(meta);
    let turn_id = data
        .interaction_id
        .as_deref()
        .and_then(|s| s.parse::<Uuid>().ok())
        .unwrap_or_else(|| deterministic_uuid(&event.id));

    let mut events = Vec::new();

    // Emit TurnCompleted with assistant response
    let tokens = data.output_tokens.map(|out| {
        json!({
            "output_tokens": out,
        })
    });

    events.push(json!({
        "type": "turn_completed",
        "session_id": session_id.to_string(),
        "turn_id": turn_id.to_string(),
        "user_message": null,
        "assistant_response": data.content,
        "model_name": null,
        "tokens": tokens,
        "duration_ms": null,
        "status": "completed",
    }));

    // Emit ThinkingBlock for reasoning text
    if let Some(reasoning) = &data.reasoning_text
        && !reasoning.is_empty()
    {
        events.push(json!({
            "type": "thinking_block",
            "turn_id": turn_id.to_string(),
            "content": reasoning,
        }));
    }

    // Emit ToolCallStarted for each tool request
    if let Some(tool_requests) = &data.tool_requests {
        for req in tool_requests {
            let tool_call_id = req
                .tool_call_id
                .as_deref()
                .and_then(|s| s.parse::<Uuid>().ok())
                .unwrap_or_else(|| {
                    deterministic_uuid(req.tool_call_id.as_deref().unwrap_or("unknown"))
                });

            let tool_name = req.name.as_deref().unwrap_or("unknown");

            events.push(json!({
                "type": "tool_call_started",
                "turn_id": turn_id.to_string(),
                "effect_id": tool_call_id.to_string(),
                "name": tool_name,
                "arguments": req.arguments,
            }));
        }
    }

    events
}

// ── Tool events ──

#[allow(clippy::too_many_lines)]
fn map_tool_start(event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let data: parser::ToolStartData =
        serde_json::from_value(event.data.clone()).unwrap_or_default();

    let turn_id = turn_uuid_from_event(event, meta);
    let tool_name = data.tool_name.as_deref().unwrap_or("unknown");
    let effect_id = data
        .tool_call_id
        .as_deref()
        .and_then(|s| s.parse::<Uuid>().ok())
        .unwrap_or_else(|| deterministic_uuid(data.tool_call_id.as_deref().unwrap_or(&event.id)));

    let mut events = vec![json!({
        "type": "tool_call_started",
        "turn_id": turn_id.to_string(),
        "effect_id": effect_id.to_string(),
        "name": tool_name,
        "arguments": data.arguments,
    })];

    // Emit specialized events based on tool name
    match tool_name {
        "view" | "read" | "read_file" => {
            if let Some(path) = data
                .arguments
                .as_ref()
                .and_then(|a| a.get("path"))
                .and_then(|p| p.as_str())
            {
                events.push(json!({
                    "type": "file_read",
                    "turn_id": turn_id.to_string(),
                    "path": path,
                }));
            }
        }
        "edit" | "write" | "write_file" => {
            if let Some(path) = data
                .arguments
                .as_ref()
                .and_then(|a| a.get("path"))
                .and_then(|p| p.as_str())
            {
                events.push(json!({
                    "type": "file_written",
                    "turn_id": turn_id.to_string(),
                    "path": path,
                }));
            }
        }
        "create" | "create_file" => {
            if let Some(path) = data
                .arguments
                .as_ref()
                .and_then(|a| a.get("path"))
                .and_then(|p| p.as_str())
            {
                events.push(json!({
                    "type": "file_created",
                    "turn_id": turn_id.to_string(),
                    "path": path,
                }));
            }
        }
        "powershell" | "bash" | "shell" | "terminal" | "read_powershell" | "write_powershell"
        | "stop_powershell" => {
            let command = data
                .arguments
                .as_ref()
                .and_then(|a| a.get("command"))
                .and_then(|c| c.as_str())
                .unwrap_or("(unknown command)")
                .to_string();
            let cwd = data
                .arguments
                .as_ref()
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
        "grep" | "glob" | "search" | "find" => {
            let query = data
                .arguments
                .as_ref()
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
        "web_fetch" | "fetch" => {
            let url = data
                .arguments
                .as_ref()
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
        "report_intent" => {
            let intent = data
                .arguments
                .as_ref()
                .and_then(|a| a.get("intent"))
                .and_then(|i| i.as_str())
                .unwrap_or("")
                .to_string();

            events.push(json!({
                "type": "intent_declared",
                "turn_id": turn_id.to_string(),
                "intent": intent,
            }));
        }
        "task" => {
            let agent_name = data
                .arguments
                .as_ref()
                .and_then(|a| a.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("unknown")
                .to_string();
            let agent_type = data
                .arguments
                .as_ref()
                .and_then(|a| a.get("agent_type"))
                .and_then(|t| t.as_str())
                .unwrap_or("general-purpose")
                .to_string();
            let mode = data
                .arguments
                .as_ref()
                .and_then(|a| a.get("mode"))
                .and_then(|m| m.as_str())
                .unwrap_or("sync")
                .to_string();

            events.push(json!({
                "type": "sub_agent_spawned",
                "turn_id": turn_id.to_string(),
                "effect_id": effect_id.to_string(),
                "agent_name": agent_name,
                "agent_type": agent_type,
                "mode": mode,
            }));
        }
        _ => {}
    }

    events
}

fn map_tool_complete(event: &RawEvent) -> Vec<serde_json::Value> {
    let data: parser::ToolCompleteData =
        serde_json::from_value(event.data.clone()).unwrap_or_default();

    let effect_id = data
        .tool_call_id
        .as_deref()
        .and_then(|s| s.parse::<Uuid>().ok())
        .unwrap_or_else(|| deterministic_uuid(data.tool_call_id.as_deref().unwrap_or(&event.id)));

    let status = if data.success.unwrap_or(false) {
        "succeeded"
    } else if data.error.is_some() {
        "failed"
    } else {
        "succeeded"
    };

    let result_data = if data.result.is_some() || data.tool_telemetry.is_some() {
        Some(json!({
            "result": data.result,
            "telemetry": data.tool_telemetry,
            "error": data.error,
        }))
    } else {
        None
    };

    vec![json!({
        "type": "tool_call_completed",
        "effect_id": effect_id.to_string(),
        "status": status,
        "result": result_data,
        "duration_ms": null,
    })]
}

// ── Sub-agent events ──

fn map_subagent_started(event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let data: parser::SubagentStartedData =
        serde_json::from_value(event.data.clone()).unwrap_or_default();

    let turn_id = turn_uuid_from_event(event, meta);
    let effect_id = data
        .tool_call_id
        .as_deref()
        .and_then(|s| s.parse::<Uuid>().ok())
        .unwrap_or_else(|| deterministic_uuid(data.tool_call_id.as_deref().unwrap_or(&event.id)));

    vec![json!({
        "type": "sub_agent_spawned",
        "turn_id": turn_id.to_string(),
        "effect_id": effect_id.to_string(),
        "agent_type": data.agent_name.unwrap_or("unknown".into()),
        "prompt": data.agent_description,
    })]
}

fn map_subagent_completed(event: &RawEvent) -> Vec<serde_json::Value> {
    let data: parser::SubagentEndData =
        serde_json::from_value(event.data.clone()).unwrap_or_default();

    let effect_id = data
        .tool_call_id
        .as_deref()
        .and_then(|s| s.parse::<Uuid>().ok())
        .unwrap_or_else(|| deterministic_uuid(data.tool_call_id.as_deref().unwrap_or(&event.id)));

    vec![json!({
        "type": "sub_agent_completed",
        "effect_id": effect_id.to_string(),
        "status": "completed",
        "duration_ms": null,
    })]
}

fn map_subagent_failed(event: &RawEvent) -> Vec<serde_json::Value> {
    let data: parser::SubagentEndData =
        serde_json::from_value(event.data.clone()).unwrap_or_default();

    let effect_id = data
        .tool_call_id
        .as_deref()
        .and_then(|s| s.parse::<Uuid>().ok())
        .unwrap_or_else(|| deterministic_uuid(data.tool_call_id.as_deref().unwrap_or(&event.id)));

    vec![json!({
        "type": "sub_agent_completed",
        "effect_id": effect_id.to_string(),
        "status": "failed",
        "duration_ms": null,
    })]
}

// ── Hooks / Skills ──

fn map_hook_start(event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let data: parser::HookStartData =
        serde_json::from_value(event.data.clone()).unwrap_or_default();

    let session_id = session_uuid(meta);
    let hook_id = data
        .hook_invocation_id
        .as_deref()
        .and_then(|s| s.parse::<Uuid>().ok())
        .unwrap_or_else(|| deterministic_uuid(&event.id));

    let tool_name = data
        .input
        .as_ref()
        .and_then(|i| i.get("toolName"))
        .and_then(|t| t.as_str())
        .map(String::from);

    vec![json!({
        "type": "hook_started",
        "session_id": session_id.to_string(),
        "hook_id": hook_id.to_string(),
        "hook_type": data.hook_type.unwrap_or("unknown".into()),
        "tool_name": tool_name,
    })]
}

fn map_hook_end(event: &RawEvent) -> Vec<serde_json::Value> {
    let data: parser::HookEndData = serde_json::from_value(event.data.clone()).unwrap_or_default();

    let hook_id = data
        .hook_invocation_id
        .as_deref()
        .and_then(|s| s.parse::<Uuid>().ok())
        .unwrap_or_else(|| deterministic_uuid(&event.id));

    vec![json!({
        "type": "hook_completed",
        "hook_id": hook_id.to_string(),
        "success": data.success.unwrap_or(true),
    })]
}

fn map_skill_invoked(event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let data: parser::SkillInvokedData =
        serde_json::from_value(event.data.clone()).unwrap_or_default();

    let turn_id = turn_uuid_from_event(event, meta);

    vec![json!({
        "type": "skill_invoked",
        "turn_id": turn_id.to_string(),
        "name": data.name.unwrap_or("unknown".into()),
        "path": data.path,
    })]
}

// ── System events ──

fn map_system_notification(event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let data: parser::SystemNotificationData =
        serde_json::from_value(event.data.clone()).unwrap_or_default();

    let session_id = session_uuid(meta);

    vec![json!({
        "type": "custom",
        "event_type": "system_notification",
        "data": {
            "content": data.content,
            "kind": data.kind,
            "session_id": session_id.to_string(),
        },
    })]
}

fn map_abort(_event: &RawEvent, meta: &SessionMeta) -> Vec<serde_json::Value> {
    let session_id = session_uuid(meta);

    vec![json!({
        "type": "session_ended",
        "session_id": session_id.to_string(),
        "status": "cancelled",
        "duration_ms": null,
    })]
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
    parse_or_derive_uuid(&meta.dir_id).unwrap_or_else(Uuid::new_v4)
}

/// Derive a turn UUID from event context (parent chain or event ID).
fn turn_uuid_from_event(event: &RawEvent, meta: &SessionMeta) -> Uuid {
    // Use parentId chain to find the interaction context
    event
        .parent_id
        .as_deref()
        .and_then(|pid| pid.parse::<Uuid>().ok())
        .unwrap_or_else(|| deterministic_uuid(&format!("{}:{}", meta.dir_id, event.id)))
}

/// Infer model provider from model name.
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

    /// Mapper tests share global statics (`BOOTSTRAPPED_SESSIONS`, `LAST_MODEL`, `TURN_INDICES`),
    /// so they must run serially. Recover from poison so one failing test doesn't cascade.
    static TEST_LOCK: TestMutex<()> = TestMutex::new(());

    fn lock_tests() -> std::sync::MutexGuard<'static, ()> {
        TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn default_meta() -> SessionMeta {
        SessionMeta {
            dir_id: "test-session-123".into(),
            cwd: Some("D:\\project".into()),
            git_root: None,
            branch: Some("main".into()),
            summary: None,
        }
    }

    #[test]
    fn test_map_session_start() {
        let _guard = lock_tests();
        reset_state();
        let event = RawEvent {
            event_type: "session.start".into(),
            data: serde_json::json!({
                "copilotVersion": "1.0.7",
                "producer": "agency",
                "sessionId": "abc-123",
                "startTime": "2026-01-01T00:00:00Z",
            }),
            id: "evt-1".into(),
            timestamp: "2026-01-01T00:00:00Z".into(),
            parent_id: None,
        };

        let meta = default_meta();
        let events = map_event(&event, &meta);

        assert!(events.len() >= 2);
        assert_eq!(events[0]["type"], "agent_discovered");
        assert_eq!(events[1]["type"], "session_started");
        assert_eq!(events[0]["agent_type"], "cli");
    }

    #[test]
    fn test_map_user_message() {
        let _guard = lock_tests();
        reset_state();
        let meta = default_meta();
        // Pre-bootstrap so we only test the user_message mapping.
        bootstrap_session_events(session_uuid(&meta), &meta);

        let event = RawEvent {
            event_type: "user.message".into(),
            data: serde_json::json!({
                "content": "Fix the bug",
                "interactionId": "11111111-1111-1111-1111-111111111111",
            }),
            id: "evt-2".into(),
            timestamp: "2026-01-01T00:00:01Z".into(),
            parent_id: Some("evt-1".into()),
        };

        let meta = default_meta();
        let events = map_event(&event, &meta);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["type"], "user_message");
        assert_eq!(events[0]["content"], "Fix the bug");
    }

    #[test]
    fn test_map_tool_start_grep() {
        let _guard = lock_tests();
        reset_state();
        let meta = default_meta();
        bootstrap_session_events(session_uuid(&meta), &meta);

        let event = RawEvent {
            event_type: "tool.execution_start".into(),
            data: serde_json::json!({
                "toolCallId": "22222222-2222-2222-2222-222222222222",
                "toolName": "grep",
                "arguments": {"pattern": "fn main"},
            }),
            id: "evt-3".into(),
            timestamp: "2026-01-01T00:00:02Z".into(),
            parent_id: Some("evt-2".into()),
        };

        let meta = default_meta();
        let events = map_event(&event, &meta);

        // Should produce ToolCallStarted + SearchPerformed
        assert!(events.len() >= 2);
        assert_eq!(events[0]["type"], "tool_call_started");
        assert_eq!(events[0]["name"], "grep");
        assert_eq!(events[1]["type"], "search_performed");
        assert_eq!(events[1]["query"], "fn main");
    }

    #[test]
    fn test_map_shutdown_with_model_metrics() {
        let _guard = lock_tests();
        reset_state();
        let event = RawEvent {
            event_type: "session.shutdown".into(),
            data: serde_json::json!({
                "shutdownType": "routine",
                "totalPremiumRequests": 5,
                "modelMetrics": {
                    "claude-opus-4.6": {
                        "requests": {"count": 10, "cost": 5},
                        "usage": {
                            "inputTokens": 1000,
                            "outputTokens": 500,
                            "cacheReadTokens": 800,
                        },
                    }
                },
                "codeChanges": {
                    "linesAdded": 100,
                    "linesRemoved": 20,
                    "filesModified": ["foo.rs"],
                },
            }),
            id: "evt-4".into(),
            timestamp: "2026-01-01T00:01:00Z".into(),
            parent_id: None,
        };

        let meta = default_meta();
        let events = map_event(&event, &meta);

        // Should have: session_ended + metadata_updated + model_used + token_usage + custom(request_count)
        let types: Vec<_> = events.iter().map(|e| e["type"].as_str().unwrap()).collect();
        assert!(types.contains(&"session_ended"));
        assert!(types.contains(&"session_metadata_updated"));
        assert!(types.contains(&"model_used"));

        let model_used = events.iter().find(|e| e["type"] == "model_used").unwrap();
        assert_eq!(model_used["name"], "claude-opus-4.6");
        assert_eq!(model_used["provider"], "anthropic");
    }

    #[test]
    fn test_infer_provider() {
        let _guard = lock_tests();
        assert_eq!(infer_provider("claude-opus-4.6"), Some("anthropic".into()));
        assert_eq!(infer_provider("gpt-4o"), Some("openai".into()));
        assert_eq!(infer_provider("gemini-pro"), Some("google".into()));
        assert_eq!(infer_provider("custom-model"), None);
    }
}
