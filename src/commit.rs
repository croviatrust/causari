use anyhow::Result;
use chrono::Utc;

use crate::index::{self, IndexEntry};
use crate::object::{Event, Snapshot};
use crate::repo::Repo;
use crate::snapshot::snapshot_workspace;
use crate::store::Store;

// Every recorder (re record, re watch, agent hooks, the MCP server) used to
// duplicate the same critical section: resolve parent → snapshot → write
// event → move ref. This module is the single implementation, and it is
// session-aware: pass a session name and the event extends THAT timeline,
// leaving HEAD alone. That is what lets multiple agents record concurrently
// on the same repo — one session per agent, shared ancestry, no lost events.

/// Resolve the parent event for a new record.
///
/// With a session name: the session's tip if it exists, otherwise the current
/// HEAD (a brand-new session forks implicitly from where the repo is now).
/// Without: the current HEAD.
pub fn resolve_parent(repo: &Repo, session: Option<&str>) -> Result<Option<String>> {
    match session {
        Some(name) => match repo.session_head(name)? {
            Some(id) => Ok(Some(id)),
            None => repo.head_event(),
        },
        None => repo.head_event(),
    }
}

/// Pre-state for a new record: the parent's post-snapshot, or a fresh
/// baseline snapshot of the workspace when there is no parent yet.
pub fn resolve_pre_snapshot(repo: &Repo, store: &Store, parent: &Option<String>) -> Result<String> {
    match parent {
        Some(pid) => Ok(store.read_event(pid)?.post_snapshot),
        None => {
            let tree_id = snapshot_workspace(repo)?;
            store.write_snapshot(&Snapshot {
                tree: tree_id,
                created_at: Utc::now().to_rfc3339(),
            })
        }
    }
}

/// Write the event, advance the right ref, and index it.
///
/// Callers must hold the repo lock (`repo.lock()`) across parent resolution
/// AND this call, so concurrent recorders cannot orphan each other's events.
pub fn commit_event(
    repo: &Repo,
    store: &Store,
    event: &Event,
    session: Option<&str>,
) -> Result<String> {
    let id = store.write_event(event)?;
    match session {
        Some(name) => repo.update_session(name, &id)?,
        None => repo.update_head(&id)?,
    }
    // The index is a cache: failing to append must not fail the record.
    let _ = index::append(repo, &IndexEntry::from_event(&id, event));
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_repo() -> (tempfile::TempDir, Repo) {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        (tmp, repo)
    }

    fn event(parent: Option<String>, msg: &str) -> Event {
        Event {
            schema: "causari.event.v0.2".into(),
            parent,
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
            created_at: Utc::now().to_rfc3339(),
        }
    }

    #[test]
    fn commit_without_session_advances_head() {
        let (_tmp, repo) = test_repo();
        let store = Store::new(&repo);

        let parent = resolve_parent(&repo, None).unwrap();
        assert_eq!(parent, None);

        let id = commit_event(&repo, &store, &event(None, "root"), None).unwrap();
        assert_eq!(repo.head_event().unwrap().as_deref(), Some(id.as_str()));

        // The committed event must be indexed immediately.
        let indexed = index::load(&repo).unwrap();
        assert!(indexed.contains_key(&id));
    }

    #[test]
    fn commit_on_named_session_leaves_head_alone() {
        let (_tmp, repo) = test_repo();
        let store = Store::new(&repo);

        let main_id = commit_event(&repo, &store, &event(None, "on main"), None).unwrap();

        // First commit on a new session forks implicitly from HEAD …
        let parent = resolve_parent(&repo, Some("bot")).unwrap();
        assert_eq!(parent.as_deref(), Some(main_id.as_str()));

        let bot_id = commit_event(&repo, &store, &event(parent, "bot work"), Some("bot")).unwrap();

        // … and from then on the session advances independently of HEAD.
        assert_eq!(
            repo.head_event().unwrap().as_deref(),
            Some(main_id.as_str())
        );
        assert_eq!(
            repo.session_head("bot").unwrap().as_deref(),
            Some(bot_id.as_str())
        );
        assert_eq!(
            resolve_parent(&repo, Some("bot")).unwrap().as_deref(),
            Some(bot_id.as_str())
        );
    }

    #[test]
    fn pre_snapshot_falls_back_to_a_fresh_baseline() {
        let (_tmp, repo) = test_repo();
        let store = Store::new(&repo);

        // No parent: a baseline snapshot of the workspace is synthesized.
        let snap_id = resolve_pre_snapshot(&repo, &store, &None).unwrap();
        assert!(store.read_snapshot(&snap_id).is_ok());

        // With a parent: the parent's post-snapshot is reused verbatim.
        let mut ev = event(None, "root");
        ev.post_snapshot = snap_id.clone();
        let id = commit_event(&repo, &store, &ev, None).unwrap();
        let pre = resolve_pre_snapshot(&repo, &store, &Some(id)).unwrap();
        assert_eq!(pre, snap_id);
    }
}
