use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

use crate::dag;
use crate::object::Event;
use crate::repo::Repo;
use crate::store::Store;

// Commands like `re find` need the metadata of EVERY event (prompt, message,
// reasoning, tool). Reading thousands of individual object files for each
// query gets slow fast. The index is a denormalized, append-only JSONL cache
// of event metadata: one line per event, written at commit time.
//
// It is a pure cache — the object store remains the source of truth. If the
// index is missing or behind (e.g. events written by an older binary, or the
// file was deleted), `ensure` heals it by walking the DAG and appending
// whatever is missing.

/// One indexed event. Snapshots and file lists stay in the object store;
/// the index carries what text queries and timeline listings need.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
}

impl IndexEntry {
    pub fn from_event(id: &str, ev: &Event) -> Self {
        Self {
            id: id.to_string(),
            parent: ev.parent.clone(),
            created_at: ev.created_at.clone(),
            agent: ev.agent.clone(),
            model: ev.model.clone(),
            tool: ev.tool.clone(),
            message: ev.message.clone(),
            prompt: ev.prompt.clone(),
            reasoning: ev.reasoning.clone(),
        }
    }
}

pub fn index_path(repo: &Repo) -> PathBuf {
    repo.dir.join("index").join("events.jsonl")
}

/// Append one entry to the index. Failures here must never lose the event
/// itself, so callers treat this as best-effort.
pub fn append(repo: &Repo, entry: &IndexEntry) -> Result<()> {
    let path = index_path(repo);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("opening index {}", path.display()))?;
    let line = serde_json::to_string(entry)?;
    writeln!(f, "{}", line)?;
    Ok(())
}

/// Load the raw index from disk. Corrupt lines are skipped (the index is a
/// cache; `ensure` will re-add anything genuinely missing).
pub fn load(repo: &Repo) -> Result<HashMap<String, IndexEntry>> {
    let path = index_path(repo);
    let mut out = HashMap::new();
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e).with_context(|| format!("reading index {}", path.display())),
    };
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<IndexEntry>(line) {
            out.insert(entry.id.clone(), entry);
        }
    }
    Ok(out)
}

/// Load the index, healing it first: any event reachable from any session
/// tip that is not yet indexed gets appended. Returns the complete map.
pub fn ensure(repo: &Repo, store: &Store) -> Result<HashMap<String, IndexEntry>> {
    let mut indexed = load(repo)?;
    let sessions = dag::list_sessions(repo)?;

    let mut missing: Vec<IndexEntry> = Vec::new();
    for s in &sessions {
        let mut cur = s.head.clone();
        while let Some(id) = cur {
            if indexed.contains_key(&id) {
                break; // everything below this point is already indexed
            }
            let ev = store.read_event(&id)?;
            cur = ev.parent.clone();
            missing.push(IndexEntry::from_event(&id, &ev));
        }
    }

    if !missing.is_empty() {
        // Oldest first, so the file stays roughly chronological.
        missing.reverse();
        for entry in &missing {
            if indexed.contains_key(&entry.id) {
                continue; // shared ancestry hit twice across sessions
            }
            append(repo, entry)?;
            indexed.insert(entry.id.clone(), entry.clone());
        }
    }
    Ok(indexed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_repo() -> (tempfile::TempDir, Repo) {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        (tmp, repo)
    }

    fn event(parent: Option<&str>, ts: &str, msg: &str) -> Event {
        Event {
            schema: "causari.event.v0.2".into(),
            parent: parent.map(String::from),
            agent: Some("tester".into()),
            model: None,
            tool: Some("edit".into()),
            message: Some(msg.into()),
            prompt: Some(format!("please {}", msg)),
            reasoning: None,
            reads: vec![],
            writes: vec![],
            tokens_in: None,
            tokens_out: None,
            cost_usd: None,
            pre_snapshot: "pre".into(),
            post_snapshot: "post".into(),
            exit_code: None,
            created_at: ts.into(),
        }
    }

    #[test]
    fn append_and_load_roundtrip() {
        let (_tmp, repo) = test_repo();
        let ev = event(None, "2026-01-01T00:00:01Z", "first");
        let entry = IndexEntry::from_event("id-1", &ev);
        append(&repo, &entry).unwrap();

        let loaded = load(&repo).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded["id-1"].message.as_deref(), Some("first"));
        assert_eq!(loaded["id-1"].prompt.as_deref(), Some("please first"));
    }

    #[test]
    fn corrupt_lines_are_skipped() {
        let (_tmp, repo) = test_repo();
        append(
            &repo,
            &IndexEntry::from_event("ok", &event(None, "2026-01-01T00:00:01Z", "fine")),
        )
        .unwrap();
        // Simulate a torn write at the end of the file.
        let path = index_path(&repo);
        let mut raw = std::fs::read_to_string(&path).unwrap();
        raw.push_str("{\"id\": \"trunca");
        std::fs::write(&path, raw).unwrap();

        let loaded = load(&repo).unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(loaded.contains_key("ok"));
    }

    #[test]
    fn missing_index_is_rebuilt_from_the_dag() {
        let (_tmp, repo) = test_repo();
        let store = Store::new(&repo);

        let e1 = store
            .write_event(&event(None, "2026-01-01T00:00:01Z", "root"))
            .unwrap();
        let e2 = store
            .write_event(&event(Some(&e1), "2026-01-01T00:00:02Z", "bot"))
            .unwrap();
        let e3 = store
            .write_event(&event(Some(&e1), "2026-01-01T00:00:03Z", "main"))
            .unwrap();
        repo.update_session("main", &e3).unwrap();
        repo.update_session("bot", &e2).unwrap();

        // No index file exists: ensure() must rebuild all three entries.
        assert!(!index_path(&repo).exists());
        let indexed = ensure(&repo, &store).unwrap();
        assert_eq!(indexed.len(), 3);

        // And the healed file must be loadable on its own, without dups.
        let reloaded = load(&repo).unwrap();
        assert_eq!(reloaded.len(), 3);
        let lines = std::fs::read_to_string(index_path(&repo)).unwrap();
        assert_eq!(lines.lines().count(), 3);
    }

    #[test]
    fn ensure_appends_only_whats_missing() {
        let (_tmp, repo) = test_repo();
        let store = Store::new(&repo);

        let e1 = store
            .write_event(&event(None, "2026-01-01T00:00:01Z", "root"))
            .unwrap();
        repo.update_session("main", &e1).unwrap();
        ensure(&repo, &store).unwrap();

        let e2 = store
            .write_event(&event(Some(&e1), "2026-01-01T00:00:02Z", "next"))
            .unwrap();
        repo.update_session("main", &e2).unwrap();

        let indexed = ensure(&repo, &store).unwrap();
        assert_eq!(indexed.len(), 2);
        let lines = std::fs::read_to_string(index_path(&repo)).unwrap();
        assert_eq!(lines.lines().count(), 2, "e1 must not be re-appended");
    }
}
