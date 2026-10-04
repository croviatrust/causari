//! The one answer to "which recorded event introduced this line?".
//!
//! `re why`, `re trace`, `re lens` and the MCP `causari_why` tool used to
//! carry three separate implementations of this question, and they disagreed
//! on the root event. Every consumer now goes through this module, so a line
//! has exactly one origin regardless of which command asks.
//!
//! Two rules the engine encodes:
//!
//! 1. **The root event owns its snapshot.** The first record of a repo has
//!    `pre == post`; a diff finds nothing. Every line present in that snapshot
//!    is attributed to the root event, and its evidence class tells the reader
//!    what that means (the agent named on it may or may not have written it).
//! 2. **Snapshots are looked up by path, not flattened.** Reading one file out
//!    of a snapshot walks the tree by path components, so a query costs
//!    O(events × depth) instead of O(events × files).

use anyhow::{Result, anyhow};
use similar::{ChangeTag, TextDiff};
use std::path::{Component, Path, PathBuf};

use crate::object::Event;
use crate::snapshot::flatten_tree;
use crate::store::Store;

/// The blob id of `rel` inside the tree `tree_id`, if the path exists and is
/// a file. Walks the tree by components; never flattens.
pub fn lookup_blob(store: &Store, tree_id: &str, rel: &Path) -> Result<Option<String>> {
    let mut tree_id = tree_id.to_string();
    let comps: Vec<&std::ffi::OsStr> = rel
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s),
            _ => None,
        })
        .collect();
    if comps.is_empty() {
        return Ok(None);
    }
    let last = comps.len() - 1;
    for (i, comp) in comps.iter().enumerate() {
        let name = match comp.to_str() {
            Some(s) => s,
            None => return Ok(None),
        };
        let tree = store.read_tree(&tree_id)?;
        let entry = match tree.entries.get(name) {
            Some(e) => e,
            None => return Ok(None),
        };
        if i == last {
            return Ok(if entry.kind == "blob" {
                Some(entry.id.clone())
            } else {
                None
            });
        }
        if entry.kind != "tree" {
            return Ok(None);
        }
        tree_id = entry.id.clone();
    }
    Ok(None)
}

/// The text of `rel` in the snapshot `snapshot_id`, or `None` if absent.
/// Non-UTF-8 content is returned lossily: provenance is about lines.
pub fn file_in_snapshot(store: &Store, snapshot_id: &str, rel: &Path) -> Result<Option<String>> {
    let snap = store.read_snapshot(snapshot_id)?;
    match lookup_blob(store, &snap.tree, rel)? {
        Some(blob) => Ok(Some(
            String::from_utf8_lossy(&store.read_blob(&blob)?).into_owned(),
        )),
        None => Ok(None),
    }
}

/// Pre and post text of `rel` around one event: `(pre, post)`.
pub fn file_around(
    store: &Store,
    ev: &Event,
    rel: &Path,
) -> Result<(Option<String>, Option<String>)> {
    let post = file_in_snapshot(store, &ev.post_snapshot, rel)?;
    let pre = if ev.pre_snapshot == ev.post_snapshot {
        post.clone()
    } else {
        file_in_snapshot(store, &ev.pre_snapshot, rel)?
    };
    Ok((pre, post))
}

/// Did this event introduce a line with exactly this text into `rel`?
///
/// Root events (no parent) introduce every line of every file in their
/// snapshot. Other events introduce a line when it appears in post and either
/// was absent from pre or is among the inserted hunks of the pre→post diff
/// (which handles duplicated or reordered lines).
pub fn event_introduced_line(store: &Store, ev: &Event, rel: &Path, target: &str) -> Result<bool> {
    let (pre, post) = file_around(store, ev, rel)?;
    let post = match post {
        Some(p) => p,
        None => return Ok(false),
    };
    if !post.lines().any(|l| l == target) {
        return Ok(false);
    }
    if ev.parent.is_none() {
        return Ok(true);
    }
    let pre = pre.unwrap_or_default();
    if pre == post {
        return Ok(false);
    }
    if !pre.lines().any(|l| l == target) {
        return Ok(true);
    }
    Ok(TextDiff::from_lines(&pre, &post)
        .iter_all_changes()
        .any(|c| {
            c.tag() == ChangeTag::Insert && c.value().trim_end_matches(['\n', '\r']) == target
        }))
}

/// The origin of a line: the most recent event on the chain starting at
/// `head` that introduced `target` into `rel`. Returns the event id and the
/// event, plus how many events were scanned.
pub struct LineOrigin {
    pub id: String,
    pub event: Event,
}

pub fn find_line_origin(
    store: &Store,
    head: Option<&str>,
    rel: &Path,
    target: &str,
) -> Result<(Option<LineOrigin>, usize)> {
    let mut cur = head.map(String::from);
    let mut scanned = 0usize;
    while let Some(id) = cur {
        let ev = store.read_event(&id)?;
        scanned += 1;
        if event_introduced_line(store, &ev, rel, target)? {
            return Ok((Some(LineOrigin { id, event: ev }), scanned));
        }
        cur = ev.parent;
    }
    Ok((None, scanned))
}

/// The chain of event ids from the root to `head`, oldest first.
pub fn chain_to(store: &Store, head: Option<&str>) -> Result<Vec<String>> {
    let mut chain = Vec::new();
    let mut cur = head.map(String::from);
    while let Some(id) = cur {
        let ev = store.read_event(&id)?;
        chain.push(id);
        cur = ev.parent;
    }
    chain.reverse();
    Ok(chain)
}

/// Per-line owners of `rel` after replaying every event on `chain` (oldest
/// first). `None` marks a line no recorded event can account for.
///
/// This is `re lens`; `re why` is the single-line special case. Both share
/// the rules above, so they cannot disagree.
pub fn line_owners(store: &Store, chain: &[String], rel: &Path) -> Result<Vec<Option<String>>> {
    let mut owners: Vec<Option<String>> = Vec::new();
    let mut prev_content = String::new();

    for (i, id) in chain.iter().enumerate() {
        let ev = store.read_event(id)?;
        let (pre, post) = file_around(store, &ev, rel)?;
        let post = match post {
            Some(c) => c,
            None => {
                // File absent after this event: whatever ownership existed is gone.
                if !owners.is_empty() && pre.is_some() {
                    owners.clear();
                    prev_content.clear();
                }
                continue;
            }
        };
        let pre = pre.unwrap_or_default();

        if ev.parent.is_none() {
            owners = vec![Some(id.clone()); post.lines().count()];
            prev_content = post;
            continue;
        }
        // A rename between events: this path is new, exactly one other path
        // disappeared, and its lines still match. Carry those owners by line
        // text onto the pre-state. A line the human or a formatter changed
        // stays unknown; it is not given to this event.
        if prev_content.is_empty() && !pre.is_empty() {
            if let Some((donor_owners, donor_text)) = renamed_from(store, &chain[..i], &ev, rel)? {
                owners = carry_owners(&donor_owners, &donor_text, &pre);
                prev_content = pre.clone();
            }
        }
        if pre == post {
            continue;
        }
        owners = replay_diff(&owners, &pre, &post, id, &prev_content);
        prev_content = post;
    }
    Ok(owners)
}

/// The path this file was renamed from, when the previous event's snapshot
/// has exactly one file that this event's pre-snapshot no longer has, and
/// `rel` is one of the paths that appeared. Returns that file's owners on
/// the chain so far and its text. More than one disappeared path is not a
/// rename we can name, so the lines stay unknown rather than guessed.
fn renamed_from(
    store: &Store,
    chain_before: &[String],
    ev: &Event,
    rel: &Path,
) -> Result<Option<(Vec<Option<String>>, String)>> {
    let Some(prev_id) = chain_before.last() else {
        return Ok(None);
    };
    let prev = store.read_event(prev_id)?;
    let prev_files = flatten_tree(store, &store.read_snapshot(&prev.post_snapshot)?.tree)?;
    let pre_files = flatten_tree(store, &store.read_snapshot(&ev.pre_snapshot)?.tree)?;
    let gone: Vec<&Path> = prev_files
        .keys()
        .filter(|p| !pre_files.contains_key(*p))
        .map(PathBuf::as_path)
        .collect();
    let appeared = pre_files.keys().any(|p| p == rel);
    if gone.len() != 1 || !appeared || gone[0] == rel {
        return Ok(None);
    }
    let old = gone[0];
    let owners = line_owners(store, chain_before, old)?;
    let text = file_in_snapshot(store, &prev.post_snapshot, old)?.unwrap_or_default();
    Ok(Some((owners, text)))
}

/// Owners of `pre`'s lines taken from a renamed file, by exact line text.
/// A line that is not in the donor stays unknown.
fn carry_owners(
    donor_owners: &[Option<String>],
    donor_text: &str,
    pre: &str,
) -> Vec<Option<String>> {
    let donor_lines: Vec<&str> = donor_text.lines().collect();
    pre.lines()
        .map(|line| {
            donor_lines
                .iter()
                .position(|l| *l == line)
                .and_then(|idx| donor_owners.get(idx).cloned().flatten())
        })
        .collect()
}

/// Replay one event's pre→post line diff on top of the existing owner map.
fn replay_diff(
    prev_owners: &[Option<String>],
    pre_content: &str,
    post_content: &str,
    event_id: &str,
    last_known_content: &str,
) -> Vec<Option<String>> {
    let pre_lines: Vec<&str> = pre_content.lines().collect();
    let last_lines: Vec<&str> = last_known_content.lines().collect();

    let mut working: Vec<Option<String>> = vec![None; pre_lines.len()];
    if last_lines == pre_lines && prev_owners.len() == pre_lines.len() {
        working.clone_from(&prev_owners.to_vec());
    } else {
        // The file changed between two recorded events without a record
        // (a human edit, a checkout). Carry owners over by line content
        // where possible; the rest are honestly unknown.
        for (i, line) in pre_lines.iter().enumerate() {
            if let Some(idx) = last_lines.iter().position(|l| l == line) {
                if let Some(o) = prev_owners.get(idx).cloned() {
                    working[i] = o;
                }
            }
        }
    }

    let diff = TextDiff::from_lines(pre_content, post_content);
    let mut result: Vec<Option<String>> = Vec::new();
    let mut pre_idx: usize = 0;
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Equal => {
                result.push(working.get(pre_idx).cloned().unwrap_or(None));
                pre_idx += 1;
            }
            ChangeTag::Delete => pre_idx += 1,
            ChangeTag::Insert => result.push(Some(event_id.to_string())),
        }
    }
    result
}

/// Parse `<file>:<line>`; the line is 1-based.
pub fn parse_spec(spec: &str) -> Result<(String, usize)> {
    let (file, line) = spec
        .rsplit_once(':')
        .ok_or_else(|| anyhow!("expected <file>:<line>, got '{}'", spec))?;
    let line_no: usize = line
        .parse()
        .map_err(|_| anyhow!("'{}' is not a valid line number", line))?;
    if line_no == 0 {
        return Err(anyhow!("line numbers start at 1"));
    }
    Ok((file.replace('\\', "/"), line_no))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commit::{commit_event, resolve_parent, resolve_pre_snapshot};
    use crate::object::Snapshot;
    use crate::repo::Repo;
    use crate::snapshot::snapshot_workspace;

    fn record(repo: &Repo, store: &Store, agent: &str, msg: &str) -> String {
        let _lock = repo.lock().unwrap();
        let parent = resolve_parent(repo, None).unwrap();
        let pre = resolve_pre_snapshot(repo, store, &parent).unwrap();
        let tree = snapshot_workspace(repo).unwrap();
        let post = store
            .write_snapshot(&Snapshot {
                tree,
                created_at: chrono::Utc::now().to_rfc3339(),
            })
            .unwrap();
        let ev = Event {
            schema: "causari.event.v0.2".into(),
            parent,
            agent: Some(agent.into()),
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
            pre_snapshot: pre,
            post_snapshot: post,
            exit_code: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            evidence: None,
            redactions: 0,
        };
        commit_event(repo, store, &ev, None).unwrap()
    }

    #[test]
    fn root_event_owns_its_snapshot_and_why_agrees_with_lens() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        let store = Store::new(&repo);
        std::fs::write(tmp.path().join("a.txt"), "one\ntwo\n").unwrap();
        let root = record(&repo, &store, "claude", "root");
        std::fs::write(tmp.path().join("a.txt"), "one\ntwo\nthree\n").unwrap();
        let second = record(&repo, &store, "gpt", "append");

        let rel = Path::new("a.txt");
        let head = repo.head_event().unwrap();
        let (o, _) = find_line_origin(&store, head.as_deref(), rel, "one").unwrap();
        assert_eq!(o.unwrap().id, root);
        let (o, _) = find_line_origin(&store, head.as_deref(), rel, "three").unwrap();
        assert_eq!(o.unwrap().id, second);

        let chain = chain_to(&store, head.as_deref()).unwrap();
        let owners = line_owners(&store, &chain, rel).unwrap();
        assert_eq!(
            owners,
            vec![Some(root.clone()), Some(root), Some(second)],
            "lens and why must attribute identically"
        );
    }

    #[test]
    fn nested_path_lookup_does_not_flatten() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        let store = Store::new(&repo);
        std::fs::create_dir_all(tmp.path().join("src/deep")).unwrap();
        std::fs::write(tmp.path().join("src/deep/m.rs"), "fn a() {}\n").unwrap();
        let id = record(&repo, &store, "x", "root");
        let ev = store.read_event(&id).unwrap();
        let text = file_in_snapshot(&store, &ev.post_snapshot, Path::new("src/deep/m.rs")).unwrap();
        assert_eq!(text.as_deref(), Some("fn a() {}\n"));
        assert!(
            file_in_snapshot(&store, &ev.post_snapshot, Path::new("src/deep"))
                .unwrap()
                .is_none()
        );
        assert!(
            file_in_snapshot(&store, &ev.post_snapshot, Path::new("nope.rs"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn duplicated_line_is_attributed_to_the_inserting_event() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        let store = Store::new(&repo);
        std::fs::write(tmp.path().join("a.txt"), "x\n").unwrap();
        let root = record(&repo, &store, "a", "root");
        std::fs::write(tmp.path().join("a.txt"), "x\nx\n").unwrap();
        let dup = record(&repo, &store, "b", "dup");
        let head = repo.head_event().unwrap();
        let chain = chain_to(&store, head.as_deref()).unwrap();
        let owners = line_owners(&store, &chain, Path::new("a.txt")).unwrap();
        assert_eq!(owners, vec![Some(root), Some(dup.clone())]);
        // why returns the most recent introducer of that text.
        let (o, _) = find_line_origin(&store, head.as_deref(), Path::new("a.txt"), "x").unwrap();
        assert_eq!(o.unwrap().id, dup);
    }

    #[test]
    fn parse_spec_handles_windows_paths_and_rejects_zero() {
        assert_eq!(parse_spec("src\\a.rs:3").unwrap(), ("src/a.rs".into(), 3));
        assert_eq!(parse_spec("C:/x/a.rs:1").unwrap(), ("C:/x/a.rs".into(), 1));
        assert!(parse_spec("a.rs:0").is_err());
        assert!(parse_spec("a.rs").is_err());
    }
}
