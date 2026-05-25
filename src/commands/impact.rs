use anyhow::{Result, anyhow};
use colored::Colorize;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use crate::cli::ImpactArgs;
use crate::object::resolve_id;
use crate::repo::Repo;
use crate::snapshot::{effective_reads, effective_writes};
use crate::store::Store;

/// One impacted event together with the dependency edges that explain why
/// it ended up in the cone. Each edge is `(file_read, tainted_source_event)`.
pub type ImpactedEvent = (String, Vec<(PathBuf, String)>);

/// `re impact <event-id>`
///
/// **The downstream causal cone.**
///
/// `re trace` looks backward: what made this code exist? `re impact` looks
/// forward: what flowed *out* of this action? It walks every later event and
/// asks "did this event depend on a file produced (directly or transitively)
/// by the source event?". The fixed point is the blast radius: the smallest
/// set of subsequent events that would not have happened the way they did,
/// had the source event been different.
///
/// Combined with `re trace`, Causari owns the full bidirectional causal graph
/// of a codebase's evolution. This enables a question no tool has answered:
/// *"if I revert event X, what else am I implicitly undoing?"*
pub fn run(args: ImpactArgs) -> Result<()> {
    let repo = Repo::discover()?;
    let store = Store::new(&repo);
    let source_id = resolve_id(&repo.objects_dir(), &args.event)?;

    let impacted = compute_impact(&repo, &store, &source_id)?;

    let source_ev = store.read_event(&source_id)?;
    println!(
        "{} impact of {}",
        "blast radius:".magenta().bold(),
        (&source_id[..10]).yellow()
    );
    if let Some(m) = &source_ev.message {
        println!("  {} {}", "event:".bright_black(), m);
    }
    if let Some(p) = &source_ev.prompt {
        let first = p.lines().next().unwrap_or("");
        let trimmed = if first.len() > 90 {
            format!("{}…", &first[..90])
        } else {
            first.to_string()
        };
        println!("  {} {}", "prompt:".bright_black(), trimmed.italic());
    }
    println!();

    if impacted.is_empty() {
        println!(
            "  {} no later event depended on anything this one produced.",
            "clean:".green().bold()
        );
        return Ok(());
    }

    println!(
        "  {} {} downstream event(s) depend on this one",
        "→".magenta(),
        impacted.len().to_string().cyan()
    );
    println!();
    for (id, deps) in &impacted {
        let ev = store.read_event(id)?;
        println!(
            "  {} {}  {}",
            "•".magenta(),
            (&id[..10]).yellow(),
            ev.message
                .as_deref()
                .unwrap_or("(no message)")
                .bright_white()
        );
        if let Some(a) = &ev.agent {
            println!("     {} {}", "agent: ".bright_black(), a.cyan());
        }
        if let Some(p) = &ev.prompt {
            let first = p.lines().next().unwrap_or("");
            let trimmed = if first.len() > 80 {
                format!("{}…", &first[..80])
            } else {
                first.to_string()
            };
            println!("     {} {}", "prompt:".bright_black(), trimmed.italic());
        }
        // Show one representative "because" path: file + which tainted ancestor produced it.
        for (shown, (file, src)) in deps.iter().enumerate() {
            if shown >= 3 {
                println!(
                    "     {} (+{} more dependencies)",
                    "↑".bright_black(),
                    deps.len() - 3
                );
                break;
            }
            let via = if src == &source_id {
                "directly".to_string()
            } else {
                format!("via {}", &src[..10])
            };
            println!(
                "     {} read {} ({})",
                "↑".bright_black(),
                file.display().to_string().cyan(),
                via.bright_black()
            );
        }
        println!();
    }
    Ok(())
}

/// Returns the list of impacted events, in chronological order (oldest first
/// after the source). Each entry comes with the list of (file, tainted_src) pairs
/// that explains *why* it's in the cone.
pub fn compute_impact(repo: &Repo, store: &Store, source_id: &str) -> Result<Vec<ImpactedEvent>> {
    // 1. Build the linear ancestry from HEAD; reverse to oldest-first.
    let head = repo.head_event()?;
    let mut chain: Vec<String> = Vec::new();
    let mut cur = head;
    while let Some(id) = cur {
        let ev = store.read_event(&id)?;
        chain.push(id.clone());
        cur = ev.parent;
    }
    chain.reverse();

    let src_pos = chain.iter().position(|x| x == source_id).ok_or_else(|| {
        anyhow!(
            "event {} is not in the current branch's ancestry",
            &source_id[..10]
        )
    })?;

    // 2. Initialize tainted_files = files written by source event, attributed to source.
    let source_ev = store.read_event(source_id)?;
    let mut tainted_files: HashMap<PathBuf, String> = HashMap::new();
    for f in effective_writes(store, &source_ev.pre_snapshot, &source_ev.post_snapshot)? {
        tainted_files.insert(f, source_id.to_string());
    }

    // 3. Walk forward, propagating taint.
    let mut impacted: Vec<ImpactedEvent> = Vec::new();
    for later_id in &chain[src_pos + 1..] {
        let later_ev = store.read_event(later_id)?;
        let reads = effective_reads(store, &later_ev)?;
        // Deduplicate reads with a BTreeSet for stable output.
        let reads: BTreeSet<PathBuf> = reads.into_iter().collect();

        let mut deps: Vec<(PathBuf, String)> = Vec::new();
        for f in &reads {
            if let Some(src) = tainted_files.get(f) {
                deps.push((f.clone(), src.clone()));
            }
        }

        if !deps.is_empty() {
            impacted.push((later_id.clone(), deps));
            // Propagate: this event's writes become tainted too.
            for f in effective_writes(store, &later_ev.pre_snapshot, &later_ev.post_snapshot)? {
                tainted_files.insert(f, later_id.clone());
            }
        }
    }

    Ok(impacted)
}
