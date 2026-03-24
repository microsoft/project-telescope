//! Scanner — discovers and reads Copilot JSONL session files.
//!
//! Scans `~/.copilot/session-state/*/events.jsonl` for new events,
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

/// Pre-seed file positions to the current size of every existing `events.jsonl`.
///
/// Called at startup so the first `collect()` only picks up lines appended
/// **after** the collector started — historical data is skipped.  Session
/// directories created later (not yet on disk) will have no entry in the map,
/// so `read_new_lines` falls back to offset 0 and reads them from the start.
pub fn snapshot_existing_positions(base_dir: &Path) {
    let Ok(entries) = fs::read_dir(base_dir) else {
        return;
    };

    let Ok(mut map) = FILE_POSITIONS.lock() else {
        return;
    };

    for entry in entries.filter_map(Result::ok) {
        if !entry.file_type().is_ok_and(|ft| ft.is_dir()) {
            continue;
        }

        let events_path = entry.path().join("events.jsonl");
        if let Ok(meta) = fs::metadata(&events_path) {
            map.insert(events_path, meta.len());
        }
    }
}

/// Metadata parsed from `workspace.yaml` alongside each session.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionMeta {
    /// Session UUID from the directory name.
    pub dir_id: String,
    /// Working directory.
    pub cwd: Option<String>,
    /// Git repository root.
    pub git_root: Option<String>,
    /// Git branch.
    pub branch: Option<String>,
    /// Session summary.
    pub summary: Option<String>,
}

/// A batch of raw lines collected from one session file.
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionBatch {
    /// Session metadata from workspace.yaml / directory name.
    pub meta: SessionMeta,
    /// Raw JSONL lines (each is a complete JSON object).
    pub lines: Vec<String>,
}

/// Resolve the session-state directory. Respects an override (for testing)
/// or defaults to `~/.copilot/session-state`.
pub fn session_state_dir(override_dir: Option<&str>) -> Option<PathBuf> {
    if let Some(d) = override_dir {
        let p = PathBuf::from(d);
        if p.is_dir() {
            return Some(p);
        }
        return None;
    }

    dirs::home_dir().map(|h| h.join(".copilot").join("session-state"))
}

/// Scan the session-state directory and return new lines from all sessions.
///
/// Each call returns only lines added since the last call (based on byte
/// offset tracking).  Returns `None` if there is nothing new.
pub fn collect_new_events(base_dir: &Path) -> Option<Vec<SessionBatch>> {
    let entries: Vec<_> = fs::read_dir(base_dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|ft| ft.is_dir()))
        .collect();

    let mut batches = Vec::new();

    for entry in entries {
        let dir_name = entry.file_name().to_string_lossy().to_string();
        let events_path = entry.path().join("events.jsonl");

        if !events_path.is_file() {
            continue;
        }

        let lines = read_new_lines(&events_path);
        if lines.is_empty() {
            continue;
        }

        let meta = parse_workspace_yaml(&entry.path(), &dir_name);

        batches.push(SessionBatch { meta, lines });
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
        // If truncated, reset position so we re-read from start next time.
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

/// Parse `workspace.yaml` from a session directory for enrichment.
fn parse_workspace_yaml(session_dir: &Path, dir_id: &str) -> SessionMeta {
    let yaml_path = session_dir.join("workspace.yaml");
    let mut meta = SessionMeta {
        dir_id: dir_id.to_string(),
        ..Default::default()
    };

    let Ok(content) = fs::read_to_string(&yaml_path) else {
        return meta;
    };

    // Simple key: value parser — workspace.yaml is a flat YAML file.
    for line in content.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim().to_string();
            if value.is_empty() {
                continue;
            }
            match key {
                "cwd" => meta.cwd = Some(value),
                "git_root" => meta.git_root = Some(value),
                "branch" => meta.branch = Some(value),
                "summary" => meta.summary = Some(value),
                _ => {}
            }
        }
    }

    meta
}

/// Test-only lock for serializing tests that touch global `FILE_POSITIONS`.
/// Exposed at module level so `lib.rs` integration tests can also use it.
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
        let dir = std::env::temp_dir().join("telescope_test_scanner");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("events.jsonl");

        // Reset shared state to avoid interference from other tests.
        reset_positions();

        // Write initial lines
        {
            let mut f = fs::File::create(&file_path).unwrap();
            writeln!(f, r#"{{"type":"session.start","id":"1"}}"#).unwrap();
            writeln!(f, r#"{{"type":"user.message","id":"2"}}"#).unwrap();
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
            writeln!(f, r#"{{"type":"assistant.message","id":"3"}}"#).unwrap();
        }

        let lines = read_new_lines(&file_path);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("assistant.message"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_parse_workspace_yaml() {
        let _guard = lock_tests();
        let dir = std::env::temp_dir().join("telescope_test_ws_yaml");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let yaml = "id: abc-123\ncwd: D:\\myproject\nbranch: main\nsummary: Test session\n";
        fs::write(dir.join("workspace.yaml"), yaml).unwrap();

        let meta = parse_workspace_yaml(&dir, "abc-123");
        assert_eq!(meta.dir_id, "abc-123");
        assert_eq!(meta.cwd.as_deref(), Some("D:\\myproject"));
        assert_eq!(meta.branch.as_deref(), Some("main"));
        assert_eq!(meta.summary.as_deref(), Some("Test session"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_snapshot_skips_existing_data() {
        let _guard = lock_tests();
        let base = std::env::temp_dir().join("telescope_test_snapshot_skip");
        let _ = fs::remove_dir_all(&base);
        let session_dir = base.join("old-session");
        fs::create_dir_all(&session_dir).unwrap();

        let events_path = session_dir.join("events.jsonl");
        {
            let mut f = fs::File::create(&events_path).unwrap();
            writeln!(f, r#"{{"type":"session.start","id":"1"}}"#).unwrap();
            writeln!(f, r#"{{"type":"user.message","id":"2"}}"#).unwrap();
        }

        reset_positions();
        snapshot_existing_positions(&base);

        // Existing data should be skipped
        let lines = read_new_lines(&events_path);
        assert!(
            lines.is_empty(),
            "Should skip pre-existing lines after snapshot"
        );

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_snapshot_picks_up_appended_data() {
        let _guard = lock_tests();
        let base = std::env::temp_dir().join("telescope_test_snapshot_append");
        let _ = fs::remove_dir_all(&base);
        let session_dir = base.join("active-session");
        fs::create_dir_all(&session_dir).unwrap();

        let events_path = session_dir.join("events.jsonl");
        {
            let mut f = fs::File::create(&events_path).unwrap();
            writeln!(f, r#"{{"type":"session.start","id":"1"}}"#).unwrap();
        }

        reset_positions();
        snapshot_existing_positions(&base);

        // Append new data after snapshot
        {
            let mut f = fs::OpenOptions::new()
                .append(true)
                .open(&events_path)
                .unwrap();
            writeln!(f, r#"{{"type":"user.message","id":"2"}}"#).unwrap();
        }

        let lines = read_new_lines(&events_path);
        assert_eq!(lines.len(), 1, "Should only return newly appended line");
        assert!(lines[0].contains("user.message"));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_snapshot_new_session_read_from_start() {
        let _guard = lock_tests();
        let base = std::env::temp_dir().join("telescope_test_snapshot_new");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();

        // Snapshot with no sessions yet
        reset_positions();
        snapshot_existing_positions(&base);

        // New session appears after snapshot
        let session_dir = base.join("new-session");
        fs::create_dir_all(&session_dir).unwrap();
        let events_path = session_dir.join("events.jsonl");
        {
            let mut f = fs::File::create(&events_path).unwrap();
            writeln!(f, r#"{{"type":"session.start","id":"1"}}"#).unwrap();
            writeln!(f, r#"{{"type":"user.message","id":"2"}}"#).unwrap();
        }

        // New session should be read from byte 0
        let lines = read_new_lines(&events_path);
        assert_eq!(
            lines.len(),
            2,
            "New session should be read fully from start"
        );

        let _ = fs::remove_dir_all(&base);
    }
}
