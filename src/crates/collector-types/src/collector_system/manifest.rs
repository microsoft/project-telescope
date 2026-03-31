// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//! Collector manifest — parsed from `collector.toml`.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// A parsed `collector.toml` manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectorManifest {
    /// Core collector identity.
    pub collector: CollectorInfo,
    /// Collector-specific defaults (passed to the collector as JSON).
    #[serde(default)]
    pub defaults: toml::Table,
}

/// Collector identity metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectorInfo {
    /// Unique collector name (e.g. "copilot-jsonl").
    pub name: String,
    /// Version string (e.g. "0.1.0").
    pub version: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
    /// Binary name (e.g. `telescope-collector-copilot-jsonl`).
    #[serde(default)]
    pub executable: Option<String>,
    /// Lifecycle model: "managed" (service spawns it) or "autonomous" (self-managed).
    #[serde(default)]
    pub lifecycle: Option<String>,
    /// Author name or organization.
    #[serde(default)]
    pub author: Option<String>,
    /// Project URL.
    #[serde(default)]
    pub url: Option<String>,
}

impl CollectorManifest {
    /// Create a minimal manifest for a remote collector (no TOML file).
    #[must_use]
    pub fn minimal(name: &str, version: &str, description: &str) -> Self {
        Self {
            collector: CollectorInfo {
                name: name.to_string(),
                version: version.to_string(),
                description: description.to_string(),
                executable: None,
                lifecycle: None,
                author: None,
                url: None,
            },
            defaults: toml::Table::new(),
        }
    }

    /// Parse a manifest from a TOML file.
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        Self::parse(&content)
    }

    /// Parse a manifest from a TOML string.
    pub fn parse(toml_str: &str) -> Result<Self> {
        toml::from_str(toml_str).context("failed to parse collector.toml")
    }

    /// Serialize the `[defaults]` section as a JSON string.
    pub fn defaults_json(&self) -> Result<String> {
        // Convert TOML table → serde_json::Value → JSON string.
        let value = toml_table_to_json(&self.defaults);
        serde_json::to_string(&value).context("failed to serialize defaults to JSON")
    }
}

/// Convert a TOML table to a `serde_json::Value`.
fn toml_table_to_json(table: &toml::Table) -> serde_json::Value {
    let map: serde_json::Map<String, serde_json::Value> = table
        .iter()
        .map(|(k, v)| (k.clone(), toml_value_to_json(v)))
        .collect();
    serde_json::Value::Object(map)
}

/// Convert a single TOML value to JSON.
fn toml_value_to_json(value: &toml::Value) -> serde_json::Value {
    match value {
        toml::Value::String(s) => serde_json::Value::String(s.clone()),
        toml::Value::Integer(i) => serde_json::json!(i),
        toml::Value::Float(f) => serde_json::json!(f),
        toml::Value::Boolean(b) => serde_json::Value::Bool(*b),
        toml::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(toml_value_to_json).collect())
        }
        toml::Value::Table(t) => toml_table_to_json(t),
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
    }
}
