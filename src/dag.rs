use anyhow::Result;
use std::collections::{HashMap, HashSet};

use crate::object::Event;
use crate::repo::Repo;
use crate::store::Store;

// The event graph is a DAG: each event has one parent, but many events can
// share the same parent (a fork point), and many tips can coexist — one per
// session. A "session" is a named ref under `.causari/refs/sessions/`, and is
// how multiple agents record concurrently on the same repo without stepping
// on each other: each agent gets its own tip, the histories share ancestry.

/// A named session ref and the event id it points to.
#[derive(Debug, Clone)]
pub struct Session {
    pub name: String,
    /// None when the ref exists but is empty (no events yet).
    pub head: Option<String>,
}

/// All sessions in the repo, sorted by name.
pub fn list_sessions(repo: &Repo) -> Result<Vec<Session>> {
    let dir = repo.sessions_dir();
    let mut out = Vec::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = match entry.file_name().to_str() {
            Some(s) => s.to_string(),
            None => continue,
        };
        let head = repo.session_head(&name)?;
        out.push(Session { name, head });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Walk a linear history from `head` back to the root.
/// Returns `(id, event)` pairs, newest first.
#[allow(dead_code)] // public DAG helper, used by tests and upcoming commands
pub fn walk(store: &Store, head: &str) -> Result<Vec<(String, Event)>> {
    let mut out = Vec::new();
    let mut cur = Some(head.to_string());
    while let Some(id) = cur {
        let ev = store.read_event(&id)?;
        cur = ev.parent.clone();
        out.push((id, ev));
    }
    Ok(out)
}

/// Walk the FULL event DAG: every event reachable from any session tip,
/// deduplicated. Returns events newest-first by `created_at`, plus a map
/// event-id → session names whose tip chain passes through it.
pub fn walk_all(repo: &Repo, store: &Store) -> Result<(Vec<(String, Event)>, DagInfo)> {
    let sessions = list_sessions(repo)?;
    let mut seen: HashSet<String> = HashSet::new();
    let mut events: Vec<(String, Event)> = Vec::new();
    let mut info = DagInfo::default();

    for s in &sessions {
        let Some(tip) = &s.head else { continue };
        info.tips.insert(tip.clone(), s.name.clone());
        let mut cur = Some(tip.clone());
        while let Some(id) = cur {
            let ev = store.read_event(&id)?;
            cur = ev.parent.clone();
            if let Some(p) = &ev.parent {
                info.children
                    .entry(p.clone())
                    .or_default()
                    .insert(id.clone());
            }
            if !seen.insert(id.clone()) {
                break; // shared ancestry already collected by another session
            }
            events.push((id, ev));
        }
    }

    // Newest first. created_at is RFC3339, so string order == time order.
    events.sort_by(|a, b| b.1.created_at.cmp(&a.1.created_at));
    Ok((events, info))
}

/// Structural information about the DAG, for display and queries.
#[derive(Debug, Default)]
pub struct DagInfo {
    /// tip event id → session name
    pub tips: HashMap<String, String>,
    /// event id → ids of its children (>1 child = fork point)
    pub children: HashMap<String, HashSet<String>>,
}

impl DagInfo {
    pub fn is_fork_point(&self, id: &str) -> bool {
        self.children.get(id).map(|c| c.len() > 1).unwrap_or(false)
    }
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
            agent: None,
            model: None,
            tool: None,
            message: Some(msg.into()),
            prompt: None,
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
    fn list_sessions_empty_repo() {
        let (_tmp, repo) = test_repo();
        let sessions = list_sessions(&repo).unwrap();
        assert!(sessions.is_empty());
    }

    #[test]
    fn walk_returns_newest_first() {
        let (_tmp, repo) = test_repo();
        let store = Store::new(&repo);

        let e1 = store
            .write_event(&event(None, "2026-01-01T00:00:01Z", "first"))
            .unwrap();
        let e2 = store
            .write_event(&event(Some(&e1), "2026-01-01T00:00:02Z", "second"))
            .unwrap();

        let chain = walk(&store, &e2).unwrap();
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[0].0, e2);
        assert_eq!(chain[1].0, e1);
    }

    #[test]
    fn walk_all_merges_sessions_and_finds_forks() {
        let (_tmp, repo) = test_repo();
        let store = Store::new(&repo);

        // main:  e1 ── e3
        // bot:   e1 ── e2     (forked from e1)
        let e1 = store
            .write_event(&event(None, "2026-01-01T00:00:01Z", "root"))
            .unwrap();
        let e2 = store
            .write_event(&event(Some(&e1), "2026-01-01T00:00:02Z", "bot work"))
            .unwrap();
        let e3 = store
            .write_event(&event(Some(&e1), "2026-01-01T00:00:03Z", "main work"))
            .unwrap();
        repo.update_session("main", &e3).unwrap();
        repo.update_session("bot", &e2).unwrap();

        let (events, info) = walk_all(&repo, &store).unwrap();

        // Shared ancestry deduplicated, newest first.
        let ids: Vec<&str> = events.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, vec![e3.as_str(), e2.as_str(), e1.as_str()]);

        assert_eq!(info.tips.get(&e3).map(String::as_str), Some("main"));
        assert_eq!(info.tips.get(&e2).map(String::as_str), Some("bot"));
        assert!(info.is_fork_point(&e1));
        assert!(!info.is_fork_point(&e2));
    }
}
