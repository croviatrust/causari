use anyhow::{Context, Result, anyhow};
use colored::Colorize;
use similar::{ChangeTag, TextDiff};
use std::path::{Path, PathBuf};

use crate::cli::WhyArgs;
use crate::object::Event;
use crate::repo::Repo;
use crate::snapshot::flatten_tree;
use crate::store::Store;

/// `re why path/to/file.rs:42`
///
/// Walks the event history backwards from HEAD until it finds the most recent
/// event whose post-snapshot introduced or modified that line of that file.
/// Prints the responsible agent, model, prompt and reasoning.
///
/// This is the "intent-addressable code" primitive: a piece of source no longer
/// just has authorship (git blame) — it has *intention* (the prompt that asked
/// for it, the model that produced it, and the context the agent had).
pub fn run(args: WhyArgs) -> Result<()> {
    let repo = Repo::discover()?;
    let store = Store::new(&repo);

    let (file_str, line_no) = parse_spec(&args.spec)?;
    let rel_path = PathBuf::from(&file_str);

    // 1. Read the current content of the file from disk to know what we're asking about.
    let abs = repo.root.join(&rel_path);
    let current =
        std::fs::read_to_string(&abs).with_context(|| format!("reading {}", abs.display()))?;
    let current_lines: Vec<&str> = current.lines().collect();
    if line_no == 0 || line_no > current_lines.len() {
        return Err(anyhow!(
            "{} only has {} lines (asked for line {})",
            file_str,
            current_lines.len(),
            line_no
        ));
    }
    let target_line = current_lines[line_no - 1].to_string();

    // 2. Walk events from HEAD backwards.
    let head = repo.head_event()?;
    let mut cur = head;
    let mut visited = 0usize;

    while let Some(id) = cur {
        let ev = store.read_event(&id)?;
        visited += 1;

        if event_introduced_line(&store, &ev, &rel_path, &target_line)? {
            print_attribution(&id, &ev, &file_str, line_no, &target_line);
            return Ok(());
        }
        cur = ev.parent;
    }

    println!(
        "{} no recorded event introduced this line ({} events scanned).",
        "not found:".yellow().bold(),
        visited
    );
    println!("  This usually means the line predates the first `re record` for this repo.");
    Ok(())
}

fn event_introduced_line(store: &Store, ev: &Event, rel: &Path, target: &str) -> Result<bool> {
    let pre_snap = store.read_snapshot(&ev.pre_snapshot)?;
    let post_snap = store.read_snapshot(&ev.post_snapshot)?;
    let pre_tree = flatten_tree(store, &pre_snap.tree)?;
    let post_tree = flatten_tree(store, &post_snap.tree)?;

    let pre_blob = pre_tree.get(rel);
    let post_blob = post_tree.get(rel);

    let post_text = match post_blob {
        Some(id) => String::from_utf8(store.read_blob(id)?).unwrap_or_default(),
        None => return Ok(false), // file did not exist after the event, can't be responsible
    };
    let pre_text = match pre_blob {
        Some(id) => String::from_utf8(store.read_blob(id)?).unwrap_or_default(),
        None => String::new(),
    };

    if pre_text == post_text {
        return Ok(false); // event did not touch this file
    }

    // Cheap structural check: does the target line text appear in post but not in pre?
    let pre_lines: std::collections::HashSet<&str> = pre_text.lines().collect();
    if !post_text.lines().any(|l| l == target) {
        return Ok(false);
    }

    // If the target line was NOT in pre at all, this event definitely introduced it.
    if !pre_lines.contains(target) {
        return Ok(true);
    }

    // Otherwise: maybe the line existed but was reordered/duplicated. Use a proper
    // diff to see if it was among the inserted chunks.
    let diff = TextDiff::from_lines(&pre_text, &post_text);
    Ok(diff
        .iter_all_changes()
        .any(|c| c.tag() == ChangeTag::Insert && c.value().trim_end_matches('\n') == target))
}

fn parse_spec(spec: &str) -> Result<(String, usize)> {
    let (file, line) = spec
        .rsplit_once(':')
        .ok_or_else(|| anyhow!("expected <file>:<line>, got '{}'", spec))?;
    let line_no: usize = line
        .parse()
        .with_context(|| format!("'{}' is not a valid line number", line))?;
    Ok((file.to_string(), line_no))
}

fn print_attribution(id: &str, ev: &Event, file: &str, line_no: usize, line: &str) {
    let header = format!("{}:{}", file, line_no);
    println!("{}", header.bold().underline());
    println!("  {}", line.bright_white());
    println!();
    println!(
        "{} {}",
        "introduced by".green().bold(),
        (&id[..10]).yellow()
    );
    if let Some(a) = &ev.agent {
        println!("  agent:     {}", a.cyan());
    }
    if let Some(m) = &ev.model {
        println!("  model:     {}", m.cyan());
    }
    if let Some(t) = &ev.tool {
        println!("  tool:      {}", t);
    }
    println!("  date:      {}", ev.created_at);
    if let Some(m) = &ev.message {
        println!("  message:   {}", m);
    }
    if let Some(p) = &ev.prompt {
        println!();
        println!("  {}", "prompt:".bright_black().italic());
        for line in p.lines() {
            println!("    {}", line);
        }
    }
    if let Some(r) = &ev.reasoning {
        println!();
        println!("  {}", "reasoning:".bright_black().italic());
        for line in r.lines().take(10) {
            println!("    {}", line);
        }
        if r.lines().count() > 10 {
            println!("    {}", "[…truncated]".bright_black());
        }
    }
}
