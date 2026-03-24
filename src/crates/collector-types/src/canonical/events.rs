//! Canonical event definitions — the typed intermediate representation.
//!
//! Each variant carries just enough data for the telescope processor to
//! create or update structured entities. Collectors produce these; the
//! telescope processor consumes them.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::provenance::Provenance;

/// A canonical event ready for the telescope processor.
///
/// Stored in the [`super::CanonicalStore`] after a collector processor
/// translates raw collector data. The telescope processor reads these
/// and promotes them into structured entities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalEvent {
    /// Auto-assigned store row ID (0 before insertion).
    pub id: i64,
    /// Which collector produced this event.
    pub collector_id: String,
    /// When this canonical event was created.
    pub created_at: DateTime<Utc>,
    /// Whether the telescope processor has consumed this event.
    pub promoted: bool,
    /// Data provenance.
    pub provenance: Provenance,
    /// The typed event payload.
    pub event: EventKind,
}

impl CanonicalEvent {
    /// Create a new canonical event (not yet stored).
    #[must_use]
    pub fn new(collector_id: String, provenance: Provenance, event: EventKind) -> Self {
        Self {
            id: 0,
            collector_id,
            created_at: Utc::now(),
            promoted: false,
            provenance,
            event,
        }
    }

    /// Returns the event kind discriminant as a string tag for indexing.
    #[must_use]
    pub fn event_type_tag(&self) -> &'static str {
        self.event.type_tag()
    }
}

// ── Event Kind ──

/// All canonical event types, grouped by domain.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventKind {
    // ── Agent ──
    /// A new agent was discovered (process scan, first MCP message, etc.).
    AgentDiscovered {
        /// Deterministic agent ID.
        agent_id: Uuid,
        /// Human-readable name.
        name: String,
        /// Agent type classification.
        agent_type: String,
        /// Executable path on disk.
        executable_path: Option<String>,
        /// Detected version.
        version: Option<String>,
    },
    /// Heartbeat — agent is still alive.
    AgentHeartbeat {
        /// Agent ID.
        agent_id: Uuid,
    },

    // ── Session ──
    /// A session started.
    SessionStarted {
        /// Session ID.
        session_id: Uuid,
        /// Agent ID.
        agent_id: Uuid,
        /// Working directory.
        cwd: Option<String>,
        /// Git repository (owner/repo).
        git_repo: Option<String>,
        /// Git branch.
        git_branch: Option<String>,
    },
    /// A session ended.
    SessionEnded {
        /// Session ID.
        session_id: Uuid,
        /// Final status.
        status: String,
        /// Duration in milliseconds.
        duration_ms: Option<u32>,
    },
    /// A session was resumed (reconnect after disconnect).
    SessionResumed {
        /// Session ID.
        session_id: Uuid,
    },
    /// Session metadata was updated (tags, git info, etc.).
    SessionMetadataUpdated {
        /// Session ID.
        session_id: Uuid,
        /// Updated metadata fields.
        metadata: serde_json::Value,
    },

    // ── Turn ──
    /// The user sent a message.
    UserMessage {
        /// Session ID.
        session_id: Uuid,
        /// Turn ID for this user message.
        turn_id: Uuid,
        /// The user's message content.
        content: Option<String>,
    },
    /// An agent turn started (response to user).
    TurnStarted {
        /// Session ID.
        session_id: Uuid,
        /// Turn ID.
        turn_id: Uuid,
        /// Turn index (0-based).
        turn_index: u32,
        /// Model being used.
        model_name: Option<String>,
    },
    /// A turn completed.
    TurnCompleted {
        /// Session ID.
        session_id: Uuid,
        /// Turn ID.
        turn_id: Uuid,
        /// User message.
        user_message: Option<String>,
        /// Assistant response.
        assistant_response: Option<String>,
        /// Model used.
        model_name: Option<String>,
        /// Token usage.
        tokens: Option<serde_json::Value>,
        /// Duration in milliseconds.
        duration_ms: Option<u32>,
        /// Final status.
        status: String,
    },
    /// Streaming tokens in progress.
    TurnStreaming {
        /// Turn ID.
        turn_id: Uuid,
        /// Partial content so far.
        partial_content: Option<String>,
        /// Token count so far.
        tokens_so_far: Option<u64>,
    },

    // ── Tool ──
    /// A tool call started.
    ToolCallStarted {
        /// Turn ID (parent).
        turn_id: Uuid,
        /// Effect ID.
        effect_id: Uuid,
        /// Tool name.
        name: String,
        /// Tool arguments/input.
        arguments: Option<serde_json::Value>,
    },
    /// A tool call completed.
    ToolCallCompleted {
        /// Effect ID.
        effect_id: Uuid,
        /// Result status.
        status: String,
        /// Tool output/result.
        result: Option<serde_json::Value>,
        /// Duration in milliseconds.
        duration_ms: Option<u32>,
    },

    // ── File System ──
    /// A file was read.
    FileRead {
        /// Turn ID.
        turn_id: Uuid,
        /// File path.
        path: String,
    },
    /// A file was written/modified.
    FileWritten {
        /// Turn ID.
        turn_id: Uuid,
        /// File path.
        path: String,
    },
    /// A file was created.
    FileCreated {
        /// Turn ID.
        turn_id: Uuid,
        /// File path.
        path: String,
    },
    /// A file was deleted.
    FileDeleted {
        /// Turn ID.
        turn_id: Uuid,
        /// File path.
        path: String,
    },

    // ── Shell ──
    /// A shell command started.
    ShellCommandStarted {
        /// Turn ID.
        turn_id: Uuid,
        /// Effect ID.
        effect_id: Uuid,
        /// Command line.
        command: String,
        /// Working directory.
        cwd: Option<String>,
    },
    /// A shell command completed.
    ShellCommandCompleted {
        /// Effect ID.
        effect_id: Uuid,
        /// Exit code.
        exit_code: Option<i32>,
        /// Duration in milliseconds.
        duration_ms: Option<u32>,
    },

    // ── Sub-Agent ──
    /// A sub-agent was spawned.
    SubAgentSpawned {
        /// Parent turn ID.
        turn_id: Uuid,
        /// Effect ID for this sub-agent spawn.
        effect_id: Uuid,
        /// Sub-agent name/type.
        agent_type: String,
        /// Task description.
        prompt: Option<String>,
    },
    /// A sub-agent completed its work.
    SubAgentCompleted {
        /// Effect ID.
        effect_id: Uuid,
        /// Result status.
        status: String,
        /// Duration in milliseconds.
        duration_ms: Option<u32>,
    },

    // ── Planning / Reasoning ──
    /// A plan was created.
    PlanCreated {
        /// Turn ID.
        turn_id: Uuid,
        /// Plan content.
        content: String,
    },
    /// A plan step was completed.
    PlanStepCompleted {
        /// Turn ID.
        turn_id: Uuid,
        /// Step description.
        step: String,
    },
    /// A thinking/reasoning block.
    ThinkingBlock {
        /// Turn ID.
        turn_id: Uuid,
        /// Reasoning content.
        content: String,
    },

    // ── Context Window ──
    /// Context window state snapshot.
    ContextWindowSnapshot {
        /// Session ID.
        session_id: Uuid,
        /// Total tokens in context.
        total_tokens: u64,
        /// Maximum context size.
        max_tokens: Option<u64>,
    },
    /// Context was pruned to make room.
    ContextPruned {
        /// Session ID.
        session_id: Uuid,
        /// Tokens removed.
        tokens_removed: u64,
    },

    // ── Human-in-the-Loop ──
    /// Agent requested approval from the user.
    ApprovalRequested {
        /// Turn ID.
        turn_id: Uuid,
        /// What the agent wants to do.
        action: String,
    },
    /// User granted approval.
    ApprovalGranted {
        /// Turn ID.
        turn_id: Uuid,
    },
    /// User denied approval.
    ApprovalDenied {
        /// Turn ID.
        turn_id: Uuid,
        /// Reason for denial.
        reason: Option<String>,
    },
    /// User provided inline feedback.
    UserFeedback {
        /// Turn ID.
        turn_id: Uuid,
        /// Feedback content.
        content: String,
        /// Sentiment (positive/negative/neutral).
        sentiment: Option<String>,
    },

    // ── Self-Report (agent-volunteered introspection) ──
    /// Agent declared its current intent.
    IntentDeclared {
        /// Turn ID.
        turn_id: Uuid,
        /// Intent description.
        intent: String,
    },
    /// Agent reported a decision and its reasoning.
    DecisionMade {
        /// Turn ID.
        turn_id: Uuid,
        /// What was decided.
        decision: String,
        /// Why.
        reasoning: Option<String>,
        /// What alternatives were considered.
        alternatives: Option<Vec<String>>,
    },
    /// Agent logged a thought/observation.
    ThoughtLogged {
        /// Turn ID.
        turn_id: Uuid,
        /// Thought content.
        content: String,
        /// Category/tag.
        category: Option<String>,
    },
    /// Agent reported frustration with a tool or approach.
    FrustrationReported {
        /// Turn ID.
        turn_id: Uuid,
        /// What's frustrating.
        issue: String,
        /// Severity (low/medium/high).
        severity: Option<String>,
    },
    /// Agent reported the outcome of an action.
    OutcomeReported {
        /// Turn ID.
        turn_id: Uuid,
        /// What happened.
        outcome: String,
        /// Whether it was successful.
        success: bool,
    },
    /// Agent logged an observation about the environment.
    ObservationLogged {
        /// Turn ID.
        turn_id: Uuid,
        /// Observation content.
        observation: String,
    },
    /// Agent followed a recipe/playbook.
    RecipeFollowed {
        /// Turn ID.
        turn_id: Uuid,
        /// Recipe name.
        recipe: String,
    },
    /// Agent considered but rejected a path.
    PathNotTaken {
        /// Turn ID.
        turn_id: Uuid,
        /// The rejected approach.
        path: String,
        /// Why it was rejected.
        reason: String,
    },
    /// Agent made an assumption.
    AssumptionMade {
        /// Turn ID.
        turn_id: Uuid,
        /// The assumption.
        assumption: String,
    },

    // ── Model ──
    /// Model usage recorded.
    ModelUsed {
        /// Session ID.
        session_id: Uuid,
        /// Model name.
        name: String,
        /// Provider.
        provider: Option<String>,
        /// Token usage.
        tokens: Option<serde_json::Value>,
        /// Cost data (e.g., premium request cost).
        cost: Option<serde_json::Value>,
        /// Number of requests/invocations.
        invocation_count: Option<u32>,
    },
    /// Model was switched mid-session.
    ModelSwitched {
        /// Session ID.
        session_id: Uuid,
        /// Previous model.
        from_model: String,
        /// New model.
        to_model: String,
    },

    // ── Error ──
    /// An error occurred.
    ErrorOccurred {
        /// Turn ID (if within a turn).
        turn_id: Option<Uuid>,
        /// Session ID (if within a session).
        session_id: Option<Uuid>,
        /// Error message.
        message: String,
        /// Error category.
        category: Option<String>,
    },
    /// A retry was attempted.
    RetryAttempted {
        /// Turn ID.
        turn_id: Uuid,
        /// Attempt number.
        attempt: u32,
        /// Reason for retry.
        reason: Option<String>,
    },

    // ── Code ──
    /// A code search was performed.
    SearchPerformed {
        /// Turn ID.
        turn_id: Uuid,
        /// Search query/pattern.
        query: String,
        /// Number of results.
        result_count: Option<u32>,
    },
    /// A code change was applied.
    CodeChangeApplied {
        /// Turn ID.
        turn_id: Uuid,
        /// File path.
        path: String,
        /// Type of change (edit/create/delete).
        change_type: String,
    },

    // ── Network ──
    /// A web request was made.
    WebRequestMade {
        /// Turn ID.
        turn_id: Uuid,
        /// URL.
        url: String,
        /// HTTP method.
        method: Option<String>,
        /// Response status code.
        status_code: Option<u16>,
    },
    /// An MCP server was connected.
    McpServerConnected {
        /// Session ID.
        session_id: Uuid,
        /// Server name.
        server_name: String,
    },

    // ── Cost / Rate ──
    /// Token usage was reported.
    TokenUsageReported {
        /// Turn ID.
        turn_id: Uuid,
        /// Input tokens.
        input_tokens: Option<u64>,
        /// Output tokens.
        output_tokens: Option<u64>,
        /// Cache read tokens.
        cache_read_tokens: Option<u64>,
    },
    /// Rate limit was hit.
    RateLimitHit {
        /// Session ID.
        session_id: Uuid,
        /// Retry-after duration in seconds.
        retry_after_secs: Option<u32>,
    },

    // ── Git / VCS ──
    /// A git commit was created.
    GitCommitCreated {
        /// Turn ID.
        turn_id: Uuid,
        /// Commit SHA.
        sha: String,
        /// Commit message.
        message: Option<String>,
    },
    /// A git branch was created.
    GitBranchCreated {
        /// Turn ID.
        turn_id: Uuid,
        /// Branch name.
        branch: String,
    },
    /// A pull request was created.
    PullRequestCreated {
        /// Turn ID.
        turn_id: Uuid,
        /// PR number or URL.
        identifier: String,
        /// PR title.
        title: Option<String>,
    },

    // ── Session Management ──
    /// Session mode was changed (e.g., interactive → plan).
    SessionModeChanged {
        /// Session ID.
        session_id: Uuid,
        /// Previous mode.
        previous_mode: String,
        /// New mode.
        new_mode: String,
    },
    /// Context compaction (checkpointing) started.
    CompactionStarted {
        /// Session ID.
        session_id: Uuid,
    },
    /// Context compaction completed.
    CompactionCompleted {
        /// Session ID.
        session_id: Uuid,
        /// Whether compaction succeeded.
        success: bool,
        /// Tokens before compaction.
        pre_compaction_tokens: Option<u64>,
        /// Checkpoint number.
        checkpoint_number: Option<u32>,
        /// Tokens used for the compaction itself.
        compaction_tokens_used: Option<serde_json::Value>,
    },

    // ── Hooks / Skills ──
    /// A hook (extension point) started execution.
    HookStarted {
        /// Session ID.
        session_id: Uuid,
        /// Hook invocation ID.
        hook_id: Uuid,
        /// Hook type (e.g., "postToolUse", "preModelCall").
        hook_type: String,
        /// Tool being hooked.
        tool_name: Option<String>,
    },
    /// A hook completed execution.
    HookCompleted {
        /// Hook invocation ID.
        hook_id: Uuid,
        /// Whether the hook succeeded.
        success: bool,
    },
    /// A skill/plugin was invoked.
    SkillInvoked {
        /// Turn ID.
        turn_id: Uuid,
        /// Skill name.
        name: String,
        /// Skill definition path.
        path: Option<String>,
    },

    // ── Catch-all ──
    /// Custom event type for anything not covered above.
    Custom {
        /// Event type string.
        event_type: String,
        /// Arbitrary JSON data.
        data: serde_json::Value,
    },
}

impl EventKind {
    /// Returns a static string tag for the event variant.
    #[must_use]
    pub fn type_tag(&self) -> &'static str {
        match self {
            Self::AgentDiscovered { .. } => "agent_discovered",
            Self::AgentHeartbeat { .. } => "agent_heartbeat",
            Self::SessionStarted { .. } => "session_started",
            Self::SessionEnded { .. } => "session_ended",
            Self::SessionResumed { .. } => "session_resumed",
            Self::SessionMetadataUpdated { .. } => "session_metadata_updated",
            Self::UserMessage { .. } => "user_message",
            Self::TurnStarted { .. } => "turn_started",
            Self::TurnCompleted { .. } => "turn_completed",
            Self::TurnStreaming { .. } => "turn_streaming",
            Self::ToolCallStarted { .. } => "tool_call_started",
            Self::ToolCallCompleted { .. } => "tool_call_completed",
            Self::FileRead { .. } => "file_read",
            Self::FileWritten { .. } => "file_written",
            Self::FileCreated { .. } => "file_created",
            Self::FileDeleted { .. } => "file_deleted",
            Self::ShellCommandStarted { .. } => "shell_command_started",
            Self::ShellCommandCompleted { .. } => "shell_command_completed",
            Self::SubAgentSpawned { .. } => "sub_agent_spawned",
            Self::SubAgentCompleted { .. } => "sub_agent_completed",
            Self::PlanCreated { .. } => "plan_created",
            Self::PlanStepCompleted { .. } => "plan_step_completed",
            Self::ThinkingBlock { .. } => "thinking_block",
            Self::ContextWindowSnapshot { .. } => "context_window_snapshot",
            Self::ContextPruned { .. } => "context_pruned",
            Self::ApprovalRequested { .. } => "approval_requested",
            Self::ApprovalGranted { .. } => "approval_granted",
            Self::ApprovalDenied { .. } => "approval_denied",
            Self::UserFeedback { .. } => "user_feedback",
            Self::IntentDeclared { .. } => "intent_declared",
            Self::DecisionMade { .. } => "decision_made",
            Self::ThoughtLogged { .. } => "thought_logged",
            Self::FrustrationReported { .. } => "frustration_reported",
            Self::OutcomeReported { .. } => "outcome_reported",
            Self::ObservationLogged { .. } => "observation_logged",
            Self::RecipeFollowed { .. } => "recipe_followed",
            Self::PathNotTaken { .. } => "path_not_taken",
            Self::AssumptionMade { .. } => "assumption_made",
            Self::ModelUsed { .. } => "model_used",
            Self::ModelSwitched { .. } => "model_switched",
            Self::ErrorOccurred { .. } => "error_occurred",
            Self::RetryAttempted { .. } => "retry_attempted",
            Self::SearchPerformed { .. } => "search_performed",
            Self::CodeChangeApplied { .. } => "code_change_applied",
            Self::WebRequestMade { .. } => "web_request_made",
            Self::McpServerConnected { .. } => "mcp_server_connected",
            Self::TokenUsageReported { .. } => "token_usage_reported",
            Self::RateLimitHit { .. } => "rate_limit_hit",
            Self::GitCommitCreated { .. } => "git_commit_created",
            Self::GitBranchCreated { .. } => "git_branch_created",
            Self::PullRequestCreated { .. } => "pull_request_created",
            Self::SessionModeChanged { .. } => "session_mode_changed",
            Self::CompactionStarted { .. } => "compaction_started",
            Self::CompactionCompleted { .. } => "compaction_completed",
            Self::HookStarted { .. } => "hook_started",
            Self::HookCompleted { .. } => "hook_completed",
            Self::SkillInvoked { .. } => "skill_invoked",
            Self::Custom { .. } => "custom",
        }
    }
}
