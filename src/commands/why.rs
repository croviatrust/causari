use anyhow::{Context, Result, anyhow};
use colored::Colorize;
use std::path::PathBuf;

use crate::cli::WhyArgs;
use crate::object::Event;
use crate::provenance::{find_line_origin, parse_spec};
use crate::repo::Repo;
use crate::store::Store;

/// `re why path/to/file.rs:42`
///
/// Walks the ledger backwards from HEAD until it finds the most recent
/// event whose post-snapshot contains that line of that file.
/// Prints the recorded agent, model, prompt and evidence class.
///
/// A match is a ledger event. It is not a finding that the named agent
/// wrote the line, and a miss is not a finding that a human did.
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
    if line_no > current_lines.len() {
        return Err(anyhow!(
            "{} only has {} lines (asked for line {})",
            file_str,
            current_lines.len(),
            line_no
        ));
    }
    let target_line = current_lines[line_no - 1].to_string();

    // 2. One engine answers for why, trace, lens and MCP alike.
    let head = repo.head_event()?;
    let (origin, scanned) = find_line_origin(&store, head.as_deref(), &rel_path, &target_line)?;

    match origin {
        Some(o) => {
            print_attribution(&o.id, &o.event, &file_str, line_no, &target_line);
            Ok(())
        }
        None => {
            println!(
                "{} no matching ledger event for this line ({} events scanned).",
                "not found:".yellow().bold(),
                scanned
            );
            println!("  Absence from this ledger is not a finding about who edited the line.");
            Ok(())
        }
    }
}

fn print_attribution(id: &str, ev: &Event, file: &str, line_no: usize, line: &str) {
    let header = format!("{}:{}", file, line_no);
    println!("{}", header.bold().underline());
    println!("  {}", line.bright_white());
    println!();
    println!("{} {}", "ledger event".green().bold(), (&id[..10]).yellow());
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
    println!(
        "  evidence:  {}",
        match &ev.evidence {
            Some(e) => e.describe(),
            None => "unrecorded (event written by an older version)".to_string(),
        }
        .bright_black()
    );
    if ev.parent.is_none() {
        println!(
            "  {}",
            "note: root event — this line was present when recording started; the agent named above may not have written it"
                .bright_black()
        );
    }
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
