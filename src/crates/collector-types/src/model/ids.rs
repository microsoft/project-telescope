//! Typed IDs for all entities.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Namespace UUID for deterministic ID generation (UUID v5).
///
/// Pre-computed from `Uuid::new_v5(&Uuid::NAMESPACE_DNS, b"telescope.dev")`.
/// = 9923dfd5-04e9-5a68-bdd9-a615cee9186f
const TELESCOPE_NAMESPACE: Uuid = Uuid::from_bytes([
    0x99, 0x23, 0xdf, 0xd5, 0x04, 0xe9, 0x5a, 0x68, 0xbd, 0xd9, 0xa6, 0x15, 0xce, 0xe9, 0x18, 0x6f,
]);

macro_rules! typed_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(pub Uuid);

        impl $name {
            /// Generate a new random ID.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// Generate a deterministic ID from a stable key.
            ///
            /// The same key always produces the same ID (UUID v5, SHA-1).
            /// Use this for entities that should be deduplicated by identity
            /// rather than having a unique ID per observation.
            #[must_use]
            pub fn deterministic(key: &str) -> Self {
                Self(Uuid::new_v5(&TELESCOPE_NAMESPACE, key.as_bytes()))
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl From<Uuid> for $name {
            fn from(uuid: Uuid) -> Self {
                Self(uuid)
            }
        }
    };
}

typed_id!(
    /// Unique identifier for an agent.
    AgentId
);
typed_id!(
    /// Unique identifier for a session.
    SessionId
);
typed_id!(
    /// Unique identifier for a turn.
    TurnId
);
typed_id!(
    /// Unique identifier for a side effect.
    EffectId
);
typed_id!(
    /// Unique identifier for a model usage record.
    ModelId
);
typed_id!(
    /// Unique identifier for a device.
    DeviceId
);
typed_id!(
    /// OTEL trace ID.
    TraceId
);
typed_id!(
    /// OTEL span ID.
    SpanId
);
typed_id!(
    /// Correlation ID linking a user message turn to the corresponding agent work turn.
    CorrelationId
);

impl SessionId {
    /// A deterministic placeholder session ID for an agent.
    ///
    /// Use when we observe side effects but have no session context.
    /// The same agent always gets the same placeholder session ID.
    #[must_use]
    pub fn placeholder(agent_id: &AgentId) -> Self {
        Self::deterministic(&format!("placeholder-session:{agent_id}"))
    }
}

impl TurnId {
    /// A deterministic placeholder turn ID for a session.
    ///
    /// Use when we observe side effects but have no turn context.
    /// The same session always gets the same placeholder turn ID.
    #[must_use]
    pub fn placeholder(session_id: &SessionId) -> Self {
        Self::deterministic(&format!("placeholder-turn:{session_id}"))
    }
}

impl AgentId {
    /// A deterministic placeholder agent ID.
    ///
    /// Use when we observe events with no agent context.
    #[must_use]
    pub fn placeholder() -> Self {
        Self::deterministic("placeholder-agent")
    }
}
