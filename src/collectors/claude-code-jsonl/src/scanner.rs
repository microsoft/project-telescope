// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//! Scanner — discovers and reads Claude Code JSONL session files.
//!
//! Scans `~/.claude/projects/*/` for session JSONL files,
//! tracking per-file byte offsets to avoid re-reading old data.

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// Per-file read position so we only consume new lines.
static FILE_POSITIONS: std::sync::LazyLock<Mutex<HashMap<PathBuf, u64>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Reset all tracked positions (called on `start()`).
pub fn reset_positions() {
    if let Ok(mut map) = FILE_POSITIONS.lock() {
        map.clear();
    }
}

/// Pre-seed file positions to the current size of every existing session JSONL.
///
/// Called at startup so the first `collect()` only picks up lines appended
/// **after** the collector started — historical data is skipped.
pub fn snapshot_existing_positions(base_dir: &Path) {
    let Ok(entries) = fs::read_dir(base_dir) else {
        return;
    };

    let Ok(mut map) = FILE_POSITIONS.lock() else {
        return;
    };

    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            // Project directory — scan for session JSONL files inside
            if let Ok(inner) = fs::read_dir(&path) {
                for inner_entry in inner.filter_map(Result::ok) {
                    let inner_path = inner_entry.path();
                    if inner_path.extension().is_some_and(|e| e == "jsonl")
                        && inner_path.is_file()
                        && let Ok(meta) = fs::metadata(&inner_path)
                    {
                        map.insert(inner_path, meta.len());
                    }
                }
            }
        }
    }
}

/// Metadata extracted from session events for enrichment.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionMeta {
    /// Session UUID from the filename.
    pub session_id: String,
    /// Project identifier (encoded directory name).
    pub project_id: String,
    /// Working directory (extracted from first event).
    pub cwd: Option<String>,
    /// Git branch (extracted from first event).
    pub git_branch: Option<String>,
    /// Claude Code version (extracted from first event).
    pub version: Option<String>,
}

/// A batch of raw lines collected from one session file.
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionBatch {
    /// Session metadata from filename / first event.
    pub meta: SessionMeta,
    /// Raw JSONL lines (each is a complete JSON object).
    pub lines: Vec<String>,
}

/// Resolve the Claude Code projects directory. Respects an override (for testing)
/// or defaults to `~/.claude/projects`.
pub fn projects_dir(override_dir: Option<&str>) -> Option<PathBuf> {
    if let Some(d) = override_dir {
        let p = PathBuf::from(d);
        if p.is_dir() {
            return Some(p);
        }
        return None;
    }

    dirs::home_dir().map(|h| h.join(".claude").join("projects"))
}

/// Scan the projects directory and return new lines from all sessions.
///
/// Each call returns only lines added since the last call (based on byte
/// offset tracking). Returns `None` if there is nothing new.
pub fn collect_new_events(base_dir: &Path) -> Option<Vec<SessionBatch>> {
    let project_entries: Vec<_> = fs::read_dir(base_dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|ft| ft.is_dir()))
        .collect();

    let mut batches = Vec::new();

    for project_entry in project_entries {
        let project_id = project_entry.file_name().to_string_lossy().to_string();
        let project_path = project_entry.path();

        // Scan for session JSONL files in this project directory
        let Ok(session_entries) = fs::read_dir(&project_path) else {
            continue;
        };

        for session_entry in session_entries.filter_map(Result::ok) {
            let path = session_entry.path();

            // Only process .jsonl files (not directories like subagent dirs)
            if !path.is_file() || path.extension().is_none_or(|e| e != "jsonl") {
                continue;
            }

            let session_id = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();

            let lines = read_new_lines(&path);
            if lines.is_empty() {
                continue;
            }

            let meta = extract_meta_from_lines(&lines, &session_id, &project_id);

            batches.push(SessionBatch { meta, lines });
        }
    }

    if batches.is_empty() {
        None
    } else {
        Some(batches)
    }
}

/// Read new lines from a JSONL file since the last known position.
fn read_new_lines(path: &Path) -> Vec<String> {
    let Ok(mut file) = fs::File::open(path) else {
        return Vec::new();
    };

    let file_len = file.metadata().map(|m| m.len()).unwrap_or(0);

    let last_pos = FILE_POSITIONS
        .lock()
        .ok()
        .and_then(|map| map.get(path).copied())
        .unwrap_or(0);

    // File was truncated or hasn't grown — nothing to read.
    if file_len <= last_pos {
        if file_len < last_pos
            && let Ok(mut map) = FILE_POSITIONS.lock()
        {
            map.insert(path.to_path_buf(), 0);
        }
        return Vec::new();
    }

    if file.seek(SeekFrom::Start(last_pos)).is_err() {
        return Vec::new();
    }

    let mut reader = BufReader::new(&file);
    let mut lines = Vec::new();
    let mut new_pos = last_pos;
    let mut buf = String::new();

    loop {
        buf.clear();
        match reader.read_line(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                new_pos += n as u64;
                let trimmed = buf.trim().to_string();
                if !trimmed.is_empty() {
                    lines.push(trimmed);
                }
            }
        }
    }

    if let Ok(mut map) = FILE_POSITIONS.lock() {
        map.insert(path.to_path_buf(), new_pos);
    }

    lines
}

/// Extract session metadata from the first lines of a session file.
///
/// Claude Code embeds `cwd`, `sessionId`, `version`, and `gitBranch` in
/// every top-level event, so we grab them from the first parseable line.
fn extract_meta_from_lines(lines: &[String], session_id: &str, project_id: &str) -> SessionMeta {
    let mut meta = SessionMeta {
        session_id: session_id.to_string(),
        project_id: project_id.to_string(),
        ..Default::default()
    };

    for line in lines {
        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };

        if let Some(cwd) = val.get("cwd").and_then(|v| v.as_str()) {
            meta.cwd = Some(cwd.to_string());
        }
        if let Some(branch) = val.get("gitBranch").and_then(|v| v.as_str()) {
            meta.git_branch = Some(branch.to_string());
        }
        if let Some(version) = val.get("version").and_then(|v| v.as_str()) {
            meta.version = Some(version.to_string());
        }

        // Once we have all metadata, stop scanning
        if meta.cwd.is_some() && meta.git_branch.is_some() && meta.version.is_some() {
            break;
        }
    }

    meta
}

/// Test-only lock for serializing tests that touch global `FILE_POSITIONS`.
#[cfg(test)]
pub static SCANNER_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn lock_tests() -> std::sync::MutexGuard<'static, ()> {
        SCANNER_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[test]
    fn test_read_new_lines_incremental() {
        let _guard = lock_tests();
        let dir = std::env::temp_dir().join("telescope_test_claude_scanner");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("test-session.jsonl");

        reset_positions();

        // Write initial lines
        {
            let mut f = fs::File::create(&file_path).unwrap();
            writeln!(f, r#"{{"type":"user","uuid":"1"}}"#).unwrap();
            writeln!(f, r#"{{"type":"assistant","uuid":"2"}}"#).unwrap();
        }

        reset_positions();

        let lines = read_new_lines(&file_path);
        assert_eq!(lines.len(), 2);

        // Second read should return nothing
        let lines = read_new_lines(&file_path);
        assert_eq!(lines.len(), 0);

        // Append more lines
        {
            let mut f = fs::OpenOptions::new()
                .append(true)
                .open(&file_path)
                .unwrap();
            writeln!(f, r#"{{"type":"system","uuid":"3"}}"#).unwrap();
        }

        let lines = read_new_lines(&file_path);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("system"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_extract_meta_from_lines() {
        let _guard = lock_tests();
        let lines = vec![
            r#"{"type":"user","cwd":"C:\\code\\myproject","sessionId":"abc-123","version":"2.1.77","gitBranch":"main"}"#.to_string(),
        ];

        let meta = extract_meta_from_lines(&lines, "abc-123", "C--code-myproject");
        assert_eq!(meta.session_id, "abc-123");
        assert_eq!(meta.project_id, "C--code-myproject");
        assert_eq!(meta.cwd.as_deref(), Some("C:\\code\\myproject"));
        assert_eq!(meta.git_branch.as_deref(), Some("main"));
        assert_eq!(meta.version.as_deref(), Some("2.1.77"));
    }

    #[test]
    fn test_snapshot_skips_existing_data() {
        let _guard = lock_tests();
        let base = std::env::temp_dir().join("telescope_test_claude_snapshot_skip");
        let _ = fs::remove_dir_all(&base);
        let project_dir = base.join("C--code-test");
        fs::create_dir_all(&project_dir).unwrap();

        let session_path = project_dir.join("test-session.jsonl");
        {
            let mut f = fs::File::create(&session_path).unwrap();
            writeln!(f, r#"{{"type":"user","uuid":"1"}}"#).unwrap();
            writeln!(f, r#"{{"type":"assistant","uuid":"2"}}"#).unwrap();
        }

        reset_positions();
        snapshot_existing_positions(&base);

        let lines = read_new_lines(&session_path);
        assert!(
            lines.is_empty(),
            "Should skip pre-existing lines after snapshot"
        );

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_snapshot_picks_up_appended_data() {
        let _guard = lock_tests();
        let base = std::env::temp_dir().join("telescope_test_claude_snapshot_append");
        let _ = fs::remove_dir_all(&base);
        let project_dir = base.join("C--code-test");
        fs::create_dir_all(&project_dir).unwrap();

        let session_path = project_dir.join("test-session.jsonl");
        {
            let mut f = fs::File::create(&session_path).unwrap();
            writeln!(f, r#"{{"type":"user","uuid":"1"}}"#).unwrap();
        }

        reset_positions();
        snapshot_existing_positions(&base);

        // Append new data after snapshot
        {
            let mut f = fs::OpenOptions::new()
                .append(true)
                .open(&session_path)
                .unwrap();
            writeln!(f, r#"{{"type":"assistant","uuid":"2"}}"#).unwrap();
        }

        let lines = read_new_lines(&session_path);
        assert_eq!(lines.len(), 1, "Should only return newly appended line");
        assert!(lines[0].contains("assistant"));

        let _ = fs::remove_dir_all(&base);
    }
}
