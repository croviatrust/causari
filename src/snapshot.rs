use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::object::{Tree, TreeEntry};
use crate::repo::Repo;
use crate::store::Store;

/// Default ignore patterns. Kept simple on purpose for the MVP.
/// We will switch to full .gitignore semantics in a later pass.
const DEFAULT_IGNORES: &[&str] = &[
    ".causari",
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".venv",
    "__pycache__",
    ".idea",
    ".vscode",
];

fn is_ignored(rel_path: &Path) -> bool {
    rel_path.components().any(|c| match c.as_os_str().to_str() {
        Some(s) => DEFAULT_IGNORES.contains(&s),
        None => false,
    })
}

/// Build a tree object recursively from a directory.
/// Returns the tree id.
fn build_tree(store: &Store, root: &Path, dir: &Path) -> Result<String> {
    let mut entries = BTreeMap::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap_or(&path);
        if is_ignored(rel) {
            continue;
        }
        let name = match entry.file_name().to_str() {
            Some(s) => s.to_string(),
            None => continue, // skip non-utf8 names for now
        };
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            // Skip symlinks for the MVP to keep semantics simple.
            continue;
        }
        if ft.is_dir() {
            let child_id = build_tree(store, root, &path)?;
            entries.insert(
                name,
                TreeEntry {
                    kind: "tree".to_string(),
                    id: child_id,
                },
            );
        } else if ft.is_file() {
            let bytes =
                std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
            let blob_id = store.write_blob(&bytes)?;
            entries.insert(
                name,
                TreeEntry {
                    kind: "blob".to_string(),
                    id: blob_id,
                },
            );
        }
    }
    let tree = Tree { entries };
    store.write_tree(&tree)
}

/// Snapshot the working tree of `repo`. Returns the root tree id.
pub fn snapshot_workspace(repo: &Repo) -> Result<String> {
    let store = Store::new(repo);
    build_tree(&store, &repo.root, &repo.root)
}

/// Restore the working tree to match the given root tree id.
/// This is the killer feature: it deletes / restores files until the
/// workspace is byte-identical to the snapshot. Ignored paths are left alone.
pub fn restore_workspace(repo: &Repo, tree_id: &str) -> Result<RestoreReport> {
    let store = Store::new(repo);
    let mut report = RestoreReport::default();
    restore_tree(&store, &repo.root, tree_id, &mut report)?;
    // After writing, walk the actual filesystem to delete files not in target.
    cleanup_extras(&store, repo, tree_id, &mut report)?;
    Ok(report)
}

#[derive(Debug, Default)]
pub struct RestoreReport {
    pub files_written: usize,
    pub files_deleted: usize,
    pub files_unchanged: usize,
}

fn restore_tree(
    store: &Store,
    dir: &Path,
    tree_id: &str,
    report: &mut RestoreReport,
) -> Result<()> {
    let tree = store.read_tree(tree_id)?;
    std::fs::create_dir_all(dir)?;
    for (name, entry) in &tree.entries {
        let path = dir.join(name);
        match entry.kind.as_str() {
            "tree" => {
                restore_tree(store, &path, &entry.id, report)?;
            }
            "blob" => {
                let target = store.read_blob(&entry.id)?;
                let needs_write = match std::fs::read(&path) {
                    Ok(current) => current != target,
                    Err(_) => true,
                };
                if needs_write {
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&path, &target)?;
                    report.files_written += 1;
                } else {
                    report.files_unchanged += 1;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Walk the filesystem and delete any file not present in the target tree.
fn cleanup_extras(
    store: &Store,
    repo: &Repo,
    tree_id: &str,
    report: &mut RestoreReport,
) -> Result<()> {
    let target_paths = collect_paths(store, &PathBuf::new(), tree_id)?;
    let target_set: std::collections::HashSet<PathBuf> = target_paths.into_iter().collect();

    for entry in WalkDir::new(&repo.root).into_iter().filter_entry(|e| {
        let rel = e.path().strip_prefix(&repo.root).unwrap_or(e.path());
        !is_ignored(rel)
    }) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(&repo.root)
            .unwrap_or(entry.path())
            .to_path_buf();
        if rel.as_os_str().is_empty() {
            continue;
        }
        if !target_set.contains(&rel) {
            std::fs::remove_file(entry.path())
                .with_context(|| format!("removing {}", entry.path().display()))?;
            report.files_deleted += 1;
        }
    }
    Ok(())
}

fn collect_paths(store: &Store, prefix: &Path, tree_id: &str) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let tree = store.read_tree(tree_id)?;
    for (name, entry) in &tree.entries {
        let p = prefix.join(name);
        match entry.kind.as_str() {
            "blob" => out.push(p),
            "tree" => {
                let sub = collect_paths(store, &p, &entry.id)?;
                out.extend(sub);
            }
            _ => {}
        }
    }
    Ok(out)
}

/// Compute a flat map of relative path -> blob id for a given tree.
/// Useful for diffing two snapshots.
pub fn flatten_tree(store: &Store, tree_id: &str) -> Result<BTreeMap<PathBuf, String>> {
    let mut out = BTreeMap::new();
    flatten_inner(store, &PathBuf::new(), tree_id, &mut out)?;
    Ok(out)
}

fn flatten_inner(
    store: &Store,
    prefix: &Path,
    tree_id: &str,
    out: &mut BTreeMap<PathBuf, String>,
) -> Result<()> {
    let tree = store.read_tree(tree_id)?;
    for (name, entry) in &tree.entries {
        let p = prefix.join(name);
        match entry.kind.as_str() {
            "blob" => {
                out.insert(p, entry.id.clone());
            }
            "tree" => {
                flatten_inner(store, &p, &entry.id, out)?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Effective reads of an event = files declared by the agent in `reads`
/// PLUS every file the event modified (because writing a file implies reading
/// its previous contents). Returned as a deduped vector of PathBufs.
pub fn effective_reads(store: &Store, ev: &crate::object::Event) -> Result<Vec<PathBuf>> {
    let mut set: std::collections::HashSet<PathBuf> = ev
        .reads
        .iter()
        .map(|s| PathBuf::from(s.replace('\\', "/")))
        .collect();
    let writes = effective_writes(store, &ev.pre_snapshot, &ev.post_snapshot)?;
    for w in writes {
        set.insert(w);
    }
    Ok(set.into_iter().collect())
}

/// Collect the lines INSERTED between two snapshots, across all changed
/// files, capped at `cap` lines. This is the input to the capture layer's
/// correlation engine: inserted lines are searched inside recent LLM
/// completions to attribute the change to the prompt that caused it.
pub fn added_lines_between(
    store: &Store,
    pre_snapshot_id: &str,
    post_snapshot_id: &str,
    cap: usize,
) -> Result<Vec<String>> {
    use similar::{ChangeTag, TextDiff};

    let pre_snap = store.read_snapshot(pre_snapshot_id)?;
    let post_snap = store.read_snapshot(post_snapshot_id)?;
    let pre = flatten_tree(store, &pre_snap.tree)?;
    let post = flatten_tree(store, &post_snap.tree)?;

    let mut out = Vec::new();
    for (path, post_id) in &post {
        if out.len() >= cap {
            break;
        }
        let pre_id = pre.get(path);
        if pre_id == Some(post_id) {
            continue;
        }
        let post_text = String::from_utf8(store.read_blob(post_id)?).unwrap_or_default();
        let pre_text = match pre_id {
            Some(id) => String::from_utf8(store.read_blob(id)?).unwrap_or_default(),
            None => String::new(),
        };
        let diff = TextDiff::from_lines(&pre_text, &post_text);
        for change in diff.iter_all_changes() {
            if change.tag() == ChangeTag::Insert {
                out.push(change.value().trim_end_matches('\n').to_string());
                if out.len() >= cap {
                    break;
                }
            }
        }
    }
    Ok(out)
}

/// Compute the set of files that *actually changed* between the pre and post
/// snapshots of an event (additions, deletions, modifications).
///
/// This is the ground truth for "what the agent wrote", independent of what
/// the agent claimed in its `writes` field. Causari trusts the filesystem.
pub fn effective_writes(
    store: &Store,
    pre_snapshot_id: &str,
    post_snapshot_id: &str,
) -> Result<Vec<PathBuf>> {
    let pre_snap = store.read_snapshot(pre_snapshot_id)?;
    let post_snap = store.read_snapshot(post_snapshot_id)?;
    let pre = flatten_tree(store, &pre_snap.tree)?;
    let post = flatten_tree(store, &post_snap.tree)?;

    let mut changed: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();
    for (path, blob_id) in &post {
        match pre.get(path) {
            Some(pre_id) if pre_id == blob_id => {} // unchanged
            _ => {
                changed.insert(path.clone());
            }
        }
    }
    for path in pre.keys() {
        if !post.contains_key(path) {
            changed.insert(path.clone()); // deletion counts
        }
    }
    Ok(changed.into_iter().collect())
}
