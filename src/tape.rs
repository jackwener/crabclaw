use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

/// A single entry in the append-only tape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TapeEntry {
    pub id: u64,
    pub kind: String,
    pub payload: serde_json::Value,
    pub timestamp: String,
}

/// Summary information about the tape.
#[derive(Debug, Clone, Serialize)]
pub struct TapeInfo {
    pub name: String,
    pub entries: usize,
    pub anchors: usize,
    pub last_anchor: Option<String>,
    pub entries_since_last_anchor: usize,
}

/// Append-only JSONL tape store for session recording.
///
/// Aligned with bub's `FileTapeStore` + `TapeService`:
/// - Entries are persisted as one JSON object per line.
/// - IDs are monotonically increasing.
/// - Anchors mark semantic boundaries in the session.
pub struct TapeStore {
    name: String,
    path: PathBuf,
    entries: Vec<TapeEntry>,
    next_id: u64,
}

impl TapeStore {
    /// Create or open a tape at the given directory.
    pub fn open(dir: &Path, name: &str) -> std::io::Result<Self> {
        fs::create_dir_all(dir)?;
        let path = dir.join(format!("{name}.jsonl"));
        let entries = Self::read_file(&path)?;
        let next_id = entries.last().map_or(1, |e| e.id + 1);
        Ok(Self {
            name: name.to_string(),
            path,
            entries,
            next_id,
        })
    }

    /// Append an event entry.
    pub fn append_event(
        &mut self,
        kind: &str,
        payload: serde_json::Value,
    ) -> std::io::Result<&TapeEntry> {
        let entry = TapeEntry {
            id: self.next_id,
            kind: kind.to_string(),
            payload,
            timestamp: Utc::now().to_rfc3339(),
        };
        self.append_entry(entry)
    }

    /// Append a message to the tape.
    pub fn append_message(&mut self, role: &str, content: &str) -> std::io::Result<&TapeEntry> {
        let payload = serde_json::json!({
            "role": role,
            "content": content,
        });
        self.append_event("message", payload)
    }

    /// Create an anchor (semantic boundary marker).
    pub fn anchor(&mut self, name: &str, state: serde_json::Value) -> std::io::Result<&TapeEntry> {
        let payload = serde_json::json!({
            "name": name,
            "state": state,
        });
        self.append_event("anchor", payload)
    }

    /// Get all entries.
    pub fn entries(&self) -> &[TapeEntry] {
        &self.entries
    }

    /// Get tape summary information.
    pub fn info(&self) -> TapeInfo {
        let anchors: Vec<&TapeEntry> = self.entries.iter().filter(|e| e.kind == "anchor").collect();

        let last_anchor = anchors
            .last()
            .and_then(|a| a.payload.get("name"))
            .and_then(|n| n.as_str())
            .map(String::from);

        let entries_since_last_anchor = if let Some(last) = anchors.last() {
            self.entries.iter().filter(|e| e.id > last.id).count()
        } else {
            self.entries.len()
        };

        TapeInfo {
            name: self.name.clone(),
            entries: self.entries.len(),
            anchors: anchors.len(),
            last_anchor,
            entries_since_last_anchor,
        }
    }

    /// Reset the tape, optionally archiving the old data.
    pub fn reset(&mut self, archive: bool) -> std::io::Result<Option<PathBuf>> {
        let archive_path = if archive && self.path.exists() {
            let stamp = Utc::now().format("%Y%m%dT%H%M%SZ");
            let archive = self.path.with_extension(format!("jsonl.{stamp}.bak"));
            fs::rename(&self.path, &archive)?;
            Some(archive)
        } else {
            if self.path.exists() {
                fs::remove_file(&self.path)?;
            }
            None
        };

        self.entries.clear();
        self.next_id = 1;

        // Create bootstrap anchor
        self.anchor("session/start", serde_json::json!({"owner": "human"}))?;

        Ok(archive_path)
    }

    /// Ensure the tape has at least one anchor.
    pub fn ensure_bootstrap_anchor(&mut self) -> std::io::Result<()> {
        let has_anchor = self.entries.iter().any(|e| e.kind == "anchor");
        if !has_anchor {
            self.anchor("session/start", serde_json::json!({"owner": "human"}))?;
        }
        Ok(())
    }

    fn append_entry(&mut self, entry: TapeEntry) -> std::io::Result<&TapeEntry> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;

        let line = serde_json::to_string(&entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writeln!(file, "{line}")?;

        self.next_id = entry.id + 1;
        self.entries.push(entry);
        Ok(self.entries.last().unwrap())
    }

    fn read_file(path: &Path) -> std::io::Result<Vec<TapeEntry>> {
        if !path.exists() {
            return Ok(Vec::new());
        }

        let file = fs::File::open(path)?;
        let reader = BufReader::new(file);
        let mut entries = Vec::new();

        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<TapeEntry>(trimmed) {
                entries.push(entry);
            }
        }

        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn append_and_read_entries() {
        let dir = tempdir().unwrap();
        let mut tape = TapeStore::open(dir.path(), "test").unwrap();

        tape.append_event("event", serde_json::json!({"action": "start"}))
            .unwrap();
        tape.append_message("user", "hello").unwrap();

        assert_eq!(tape.entries().len(), 2);
        assert_eq!(tape.entries()[0].id, 1);
        assert_eq!(tape.entries()[0].kind, "event");
        assert_eq!(tape.entries()[1].id, 2);
        assert_eq!(tape.entries()[1].kind, "message");
        assert_eq!(tape.entries()[1].payload["role"], "user");
        assert_eq!(tape.entries()[1].payload["content"], "hello");
    }

    #[test]
    fn ids_auto_increment() {
        let dir = tempdir().unwrap();
        let mut tape = TapeStore::open(dir.path(), "test").unwrap();

        for i in 0..5 {
            tape.append_event("event", serde_json::json!({"i": i}))
                .unwrap();
        }

        let ids: Vec<u64> = tape.entries().iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn persistence_across_reopen() {
        let dir = tempdir().unwrap();

        {
            let mut tape = TapeStore::open(dir.path(), "persist").unwrap();
            tape.append_message("user", "first").unwrap();
            tape.append_message("assistant", "second").unwrap();
        }

        // Reopen the tape
        let tape = TapeStore::open(dir.path(), "persist").unwrap();
        assert_eq!(tape.entries().len(), 2);
        assert_eq!(tape.entries()[0].payload["content"], "first");
        assert_eq!(tape.entries()[1].payload["content"], "second");
    }

    #[test]
    fn reset_clears_entries() {
        let dir = tempdir().unwrap();
        let mut tape = TapeStore::open(dir.path(), "reset").unwrap();

        tape.append_message("user", "hello").unwrap();
        tape.append_message("assistant", "hi").unwrap();
        assert_eq!(tape.entries().len(), 2);

        let archive = tape.reset(false).unwrap();
        assert!(archive.is_none());
        // After reset, only the bootstrap anchor remains
        assert_eq!(tape.entries().len(), 1);
        assert_eq!(tape.entries()[0].kind, "anchor");
    }

    #[test]
    fn reset_with_archive() {
        let dir = tempdir().unwrap();
        let mut tape = TapeStore::open(dir.path(), "archive").unwrap();

        tape.append_message("user", "hello").unwrap();

        let archive = tape.reset(true).unwrap();
        assert!(archive.is_some());
        assert!(archive.unwrap().exists());
        // Only bootstrap anchor after reset
        assert_eq!(tape.entries().len(), 1);
    }

    #[test]
    fn info_reports_correct_stats() {
        let dir = tempdir().unwrap();
        let mut tape = TapeStore::open(dir.path(), "info").unwrap();

        tape.anchor("phase-0", serde_json::json!({"owner": "human"}))
            .unwrap();
        tape.append_message("user", "hello").unwrap();
        tape.append_message("assistant", "hi").unwrap();

        let info = tape.info();
        assert_eq!(info.name, "info");
        assert_eq!(info.entries, 3);
        assert_eq!(info.anchors, 1);
        assert_eq!(info.last_anchor, Some("phase-0".to_string()));
        assert_eq!(info.entries_since_last_anchor, 2);
    }

    #[test]
    fn ensure_bootstrap_anchor_creates_anchor_once() {
        let dir = tempdir().unwrap();
        let mut tape = TapeStore::open(dir.path(), "bootstrap").unwrap();

        assert_eq!(tape.entries().len(), 0);

        tape.ensure_bootstrap_anchor().unwrap();
        assert_eq!(tape.entries().len(), 1);
        assert_eq!(tape.entries()[0].kind, "anchor");

        // Calling again should not add another anchor
        tape.ensure_bootstrap_anchor().unwrap();
        assert_eq!(tape.entries().len(), 1);
    }

    #[test]
    fn corrupted_line_skipped() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("corrupt.jsonl");
        // Write a valid entry, a corrupt line, and another valid entry
        let valid1 = serde_json::to_string(&TapeEntry {
            id: 1,
            kind: "message".to_string(),
            payload: serde_json::json!({"role": "user", "content": "hi"}),
            timestamp: "2026-01-01T00:00:00Z".to_string(),
        })
        .unwrap();
        let valid2 = serde_json::to_string(&TapeEntry {
            id: 2,
            kind: "message".to_string(),
            payload: serde_json::json!({"role": "assistant", "content": "hello"}),
            timestamp: "2026-01-01T00:00:01Z".to_string(),
        })
        .unwrap();
        std::fs::write(&path, format!("{valid1}\nNOT_VALID_JSON\n{valid2}\n")).unwrap();

        let tape = TapeStore::open(dir.path(), "corrupt").unwrap();
        // Should have 2 entries, the corrupt line is skipped
        assert_eq!(tape.entries().len(), 2);
        assert_eq!(tape.entries()[0].id, 1);
        assert_eq!(tape.entries()[1].id, 2);
    }

    #[test]
    fn timestamp_is_rfc3339() {
        let dir = tempdir().unwrap();
        let mut tape = TapeStore::open(dir.path(), "ts").unwrap();
        tape.append_message("user", "hello").unwrap();

        let ts = &tape.entries()[0].timestamp;
        // Parse as RFC3339 — should succeed
        chrono::DateTime::parse_from_rfc3339(ts)
            .unwrap_or_else(|e| panic!("timestamp '{ts}' is not valid RFC3339: {e}"));
    }

    #[test]
    fn reopen_preserves_next_id() {
        let dir = tempdir().unwrap();

        {
            let mut tape = TapeStore::open(dir.path(), "idtest").unwrap();
            tape.append_message("user", "one").unwrap();
            tape.append_message("user", "two").unwrap();
            assert_eq!(tape.entries().last().unwrap().id, 2);
        }

        // Reopen and append — should continue from id=3
        let mut tape = TapeStore::open(dir.path(), "idtest").unwrap();
        tape.append_message("user", "three").unwrap();
        assert_eq!(tape.entries().last().unwrap().id, 3);
    }
}
