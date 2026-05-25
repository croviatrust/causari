use anyhow::{Context, Result, anyhow};
use colored::Colorize;
use similar::{ChangeTag, TextDiff};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::cli::LensArgs;
use crate::object::Event;
use crate::repo::Repo;
use crate::snapshot::flatten_tree;
use crate::store::Store;

/// `re lens path/to/file.rs`
///
/// Render a file with **inline per-line provenance**: each line is annotated
/// with the short id of the event that last introduced or modified it, the
/// agent that authored it, and a tiny excerpt of the prompt that caused the
/// change. Think `git blame` made for prompts, in glorious colour, ready to
/// be screenshotted.
///
/// Algorithm: walk every event from oldest to newest; for each event, diff its
/// pre vs post state of the target file and update an in-memory "line → owner"
/// map. Each insertion claims a new line for that event; each deletion frees
/// the line; modifications re-attribute. At the end, every line in the current
/// file has a known owner event.
pub fn run(args: LensArgs) -> Result<()> {
    let repo = Repo::discover()?;
    let store = Store::new(&repo);

    let rel = PathBuf::from(args.file.replace('\\', "/"));
    let abs = repo.root.join(&rel);
    let current =
        std::fs::read_to_string(&abs).with_context(|| format!("reading {}", abs.display()))?;

    // Walk events oldest -> newest.
    let head = repo.head_event()?;
    let mut chain: Vec<String> = Vec::new();
    let mut cur = head;
    while let Some(id) = cur {
        let ev = store.read_event(&id)?;
        chain.push(id.clone());
        cur = ev.parent;
    }
    chain.reverse();

    // Reconstruct line ownership by replaying every event's diff on the file.
    // We keep a Vec<Option<String>> of owner event ids parallel to the file's
    // current line list; lines we cannot attribute remain None.
    let mut owners: Vec<Option<String>> = Vec::new();
    let mut prev_content = String::new();

    for id in &chain {
        let ev = store.read_event(id)?;
        let post_content = match read_file_at(&store, &ev, &rel, /*pre=*/ false)? {
            Some(c) => c,
            None => continue, // file not present at this event
        };
        let pre_content: String =
            read_file_at(&store, &ev, &rel, /*pre=*/ true)?.unwrap_or_default();

        if pre_content == post_content {
            continue;
        }

        // Recompute owners after this event's diff.
        owners = replay_diff(&owners, &pre_content, &post_content, id, &prev_content);
        prev_content = post_content;
    }

    // Final reality check: align owners to the current on-disk content.
    let actual_lines: Vec<&str> = current.lines().collect();
    if owners.len() != actual_lines.len() {
        // The file changed on disk since the last event. Causari will still
        // annotate lines it can; missing ones get "?".
        owners.resize(actual_lines.len(), None);
    }

    // Cache event metadata for printing.
    let mut meta_cache: HashMap<String, Event> = HashMap::new();
    for o in owners.iter().flatten() {
        if !meta_cache.contains_key(o) {
            meta_cache.insert(o.clone(), store.read_event(o)?);
        }
    }

    // Assign each unique owner a distinct color from a small palette so a
    // screenshot is instantly readable.
    let palette = [
        |s: String| s.bright_cyan().to_string(),
        |s: String| s.bright_magenta().to_string(),
        |s: String| s.bright_green().to_string(),
        |s: String| s.bright_yellow().to_string(),
        |s: String| s.bright_blue().to_string(),
        |s: String| s.bright_red().to_string(),
    ];
    let mut color_of: HashMap<String, usize> = HashMap::new();
    for o in owners.iter().flatten() {
        let next = color_of.len() % palette.len();
        color_of.entry(o.clone()).or_insert(next);
    }

    // Print header with the legend.
    println!("{}", args.file.bold().underline());
    if !color_of.is_empty() {
        println!();
        println!("{}", "legend:".bright_black());
        let mut entries: Vec<(&String, &usize)> = color_of.iter().collect();
        entries.sort_by_key(|(_, c)| **c);
        for (id, c) in entries {
            let ev = meta_cache.get(id).unwrap();
            let label = format!(
                "  {}  {}  {}",
                &id[..10],
                ev.agent.as_deref().unwrap_or("?"),
                ev.message.as_deref().unwrap_or("")
            );
            println!("{}", palette[*c](label));
        }
    }
    println!();

    // Render each line: " 42 │ <colored short id> │ <colored source line>".
    let max_line = actual_lines.len();
    let pad = max_line.to_string().len();
    for (i, line) in actual_lines.iter().enumerate() {
        let lineno = format!("{:>width$}", i + 1, width = pad).bright_black();
        let (id_str, line_str) = match owners.get(i).and_then(|o| o.as_ref()) {
            Some(oid) => {
                let c = color_of[oid];
                let short = &oid[..10];
                (palette[c](short.to_string()), palette[c](line.to_string()))
            }
            None => ("?????????? ".bright_black().to_string(), line.to_string()),
        };
        println!("{} {} {} {}", lineno, "│".bright_black(), id_str, line_str);
    }
    Ok(())
}

/// Replay one event's pre→post line diff on top of the existing owner map.
/// `prev_content` is the line content the previous owner map was built against;
/// `pre_content` is what this event saw. If they differ we trust `pre_content`
/// (the snapshot is the source of truth) and start from a fresh map sized to
/// `pre_content` lines, with owners inherited where possible.
fn replay_diff(
    prev_owners: &[Option<String>],
    pre_content: &str,
    post_content: &str,
    event_id: &str,
    last_known_content: &str,
) -> Vec<Option<String>> {
    // Build the working owner vector matching `pre_content`'s line count.
    let pre_lines: Vec<&str> = pre_content.lines().collect();
    let last_lines: Vec<&str> = last_known_content.lines().collect();

    let mut working: Vec<Option<String>> = vec![None; pre_lines.len()];
    if last_lines == pre_lines && prev_owners.len() == pre_lines.len() {
        // Common fast path.
        working.clone_from(&prev_owners.to_vec());
    } else {
        // Best-effort: align owners by line content where possible.
        for (i, line) in pre_lines.iter().enumerate() {
            if let Some(idx) = last_lines.iter().position(|l| l == line) {
                if let Some(o) = prev_owners.get(idx).cloned() {
                    working[i] = o;
                }
            }
        }
    }

    // Now apply the pre→post diff, producing the new owner map.
    let diff = TextDiff::from_lines(pre_content, post_content);
    let mut result: Vec<Option<String>> = Vec::new();
    let mut pre_idx: usize = 0;
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Equal => {
                let owner = working.get(pre_idx).cloned().unwrap_or(None);
                result.push(owner);
                pre_idx += 1;
            }
            ChangeTag::Delete => {
                pre_idx += 1;
            }
            ChangeTag::Insert => {
                result.push(Some(event_id.to_string()));
            }
        }
    }
    result
}

fn read_file_at(store: &Store, ev: &Event, rel: &Path, pre: bool) -> Result<Option<String>> {
    let snap_id = if pre {
        &ev.pre_snapshot
    } else {
        &ev.post_snapshot
    };
    let snap = store.read_snapshot(snap_id)?;
    let tree = flatten_tree(store, &snap.tree)?;
    let key = PathBuf::from(rel.to_string_lossy().replace('\\', "/"));
    let blob_id = match tree.get(&key).or_else(|| tree.get(rel)) {
        Some(b) => b,
        None => return Ok(None),
    };
    let bytes = store.read_blob(blob_id)?;
    Ok(Some(
        String::from_utf8(bytes).map_err(|e| anyhow!("non-utf8 file: {}", e))?,
    ))
}
