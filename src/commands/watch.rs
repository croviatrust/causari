use anyhow::{Context, Result};
use chrono::Utc;
use colored::Colorize;
use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc::channel};
use std::time::Duration;

use crate::cli::WatchArgs;
use crate::object::{Event, Snapshot};
use crate::repo::Repo;
use crate::snapshot::snapshot_workspace;
use crate::store::Store;

/// `re watch` turns Causari into a passive recorder.
///
/// It watches the working tree and, every time a debounce window elapses with
/// any change, it snapshots the workspace and writes a new event. The intent
/// is that you launch this in a side terminal, then let *any* agent (Cursor,
/// Claude Code, Cline, Aider, a script, you) operate on the repo — Causari
/// records the timeline for free, with no integration required.
///
/// This is the bridge between Causari and the rest of the ecosystem.
pub fn run(args: WatchArgs) -> Result<()> {
    let repo = Repo::discover()?;
    let store = Store::new(&repo);
    let debounce_ms = args.debounce.unwrap_or(800);

    println!(
        "{} watching {} (debounce {}ms). Press Ctrl-C to stop.",
        "causari:".green().bold(),
        repo.root.display(),
        debounce_ms
    );
    if let Some(a) = &args.agent {
        println!("  agent tag: {}", a.cyan());
    }

    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = Arc::clone(&stop);
        ctrlc::set_handler(move || {
            stop.store(true, Ordering::SeqCst);
        })
        .context("installing ctrl-c handler")?;
    }

    let (tx, rx) = channel();
    let mut debouncer = new_debouncer(Duration::from_millis(debounce_ms), tx)
        .context("creating filesystem debouncer")?;
    debouncer
        .watcher()
        .watch(&repo.root, RecursiveMode::Recursive)
        .context("starting watcher")?;

    while !stop.load(Ordering::SeqCst) {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(Ok(events)) => {
                // Filter out changes inside .causari/ (we cause those ourselves).
                let touched: HashSet<PathBuf> = events
                    .into_iter()
                    .map(|e| e.path)
                    .filter(|p| !is_internal(&repo, p))
                    .collect();
                if touched.is_empty() {
                    continue;
                }
                record_change(&repo, &store, &args, &touched)?;
            }
            Ok(Err(errs)) => {
                eprintln!("{} watcher errors: {:?}", "warn:".yellow(), errs);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(e) => {
                eprintln!("{} channel closed: {:?}", "error:".red(), e);
                break;
            }
        }
    }

    println!("\n{} stopped.", "causari:".green().bold());
    Ok(())
}

fn is_internal(repo: &Repo, p: &std::path::Path) -> bool {
    p.starts_with(&repo.dir) || p.components().any(|c| c.as_os_str() == ".causari")
}

fn record_change(
    repo: &Repo,
    store: &Store,
    args: &WatchArgs,
    touched: &HashSet<PathBuf>,
) -> Result<()> {
    // Same logic as `re record`, simplified.
    let parent_id = repo.head_event()?;
    let pre_snapshot_id = match &parent_id {
        Some(pid) => store.read_event(pid)?.post_snapshot,
        None => {
            let tree_id = snapshot_workspace(repo)?;
            store.write_snapshot(&Snapshot {
                tree: tree_id,
                created_at: Utc::now().to_rfc3339(),
            })?
        }
    };
    let post_tree = snapshot_workspace(repo)?;
    let post_snapshot_id = store.write_snapshot(&Snapshot {
        tree: post_tree,
        created_at: Utc::now().to_rfc3339(),
    })?;

    // Note: noop touches (metadata-only changes producing identical content) are
    // already filtered upstream by content hashing in the snapshot path.

    let mut writes: Vec<String> = touched
        .iter()
        .filter_map(|p| {
            p.strip_prefix(&repo.root)
                .ok()
                .map(|r| r.to_string_lossy().replace('\\', "/").to_string())
        })
        .collect();
    writes.sort();
    writes.dedup();

    let event = Event {
        schema: "causari.event.v0.2".to_string(),
        parent: parent_id,
        agent: args.agent.clone(),
        model: args.model.clone(),
        tool: Some("watch".to_string()),
        message: Some(format!("{} file(s) changed", writes.len())),
        prompt: None,
        reasoning: None,
        reads: Vec::new(),
        writes: writes.clone(),
        tokens_in: None,
        tokens_out: None,
        cost_usd: None,
        pre_snapshot: pre_snapshot_id,
        post_snapshot: post_snapshot_id,
        exit_code: None,
        created_at: Utc::now().to_rfc3339(),
    };
    let id = store.write_event(&event)?;
    repo.update_head(&id)?;

    let preview = writes
        .iter()
        .take(3)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let extra = if writes.len() > 3 {
        format!(" (+{} more)", writes.len() - 3)
    } else {
        String::new()
    };
    println!(
        "  {} {}  {}{}",
        "•".green(),
        (&id[..10]).bright_black(),
        preview,
        extra.bright_black()
    );
    Ok(())
}
