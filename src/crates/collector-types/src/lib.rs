// This is a shared types crate — many constants and structs are part of the
// public API surface but only consumed by the service side or by collector
// authors in specific scenarios. Suppress dead_code warnings so the full
// protocol is available without noise.
#![allow(dead_code)]
//! Shared types for building Telescope collectors.
//!
//! This crate provides the public types that collector authors need:
//! canonical event definitions, typed IDs, provenance metadata,
//! IPC protocol/transport, and the collector manifest format.

pub mod canonical {
    //! Canonical event types that collectors produce.
    mod events;
    pub use events::EventKind;
}

pub mod model {
    //! Typed entity IDs used in events.
    mod ids;
    pub use ids::*;
}

pub mod provenance;

pub mod ipc {
    //! IPC protocol and transport for communicating with the Telescope service.
    mod framing;
    mod protocol;
    pub mod collector_protocol;
    mod transport;

    pub use framing::{read_frame, write_frame};
    pub use protocol::{IpcError, IpcNotification, IpcRequest, IpcResponse};
    pub use transport::{IpcChannel, IpcListener, IpcStream};
}

pub mod collector_system {
    //! Collector manifest format.
    mod manifest;
    pub use manifest::{CollectorInfo, CollectorManifest};
}
