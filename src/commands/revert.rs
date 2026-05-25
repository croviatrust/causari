use anyhow::Result;
use colored::Colorize;
use std::io::{BufRead, Write};

use crate::cli::RevertArgs;
use crate::commands::impact::compute_impact;
use crate::object::resolve_id;
use crate::repo::Repo;
use crate::snapshot::restore_workspace;
use crate::store::Store;

pub fn run(args: RevertArgs) -> Result<()> {
    let repo = Repo::discover()?;
    let store = Store::new(&repo);
    let full = resolve_id(&repo.objects_dir(), &args.id)?;
    let ev = store.read_event(&full)?;
    let target_snapshot = store.read_snapshot(&ev.pre_snapshot)?;

    // Causality-aware preview: compute the downstream blast radius of this event
    // BEFORE actually reverting. The user sees what they are implicitly undoing.
    let impacted = compute_impact(&repo, &store, &full)?;
    if !impacted.is_empty() {
        println!(
            "{} reverting {} will implicitly affect {} later event(s):",
            "causal preview:".magenta().bold(),
            (&full[..10]).yellow(),
            impacted.len().to_string().cyan()
        );
        for (id, _) in impacted.iter().take(5) {
            let later = store.read_event(id)?;
            println!(
                "   {} {}  {}",
                "↓".magenta(),
                (&id[..10]).bright_black(),
                later.message.as_deref().unwrap_or("").bright_white()
            );
        }
        if impacted.len() > 5 {
            println!("   {} (+{} more)", "↓".magenta(), impacted.len() - 5);
        }
        println!(
            "   {} those events read files this one produced; their reasoning may no longer make sense.",
            "note:".bright_black()
        );
        println!();
    }

    if !args.yes {
        print!(
            "{} this will rewrite files in {} to the state BEFORE event {}. Continue? [y/N] ",
            "warning:".yellow().bold(),
            repo.root.display(),
            (&full[..10]).yellow()
        );
        std::io::stdout().flush()?;
        let stdin = std::io::stdin();
        let mut line = String::new();
        stdin.lock().read_line(&mut line)?;
        let answer = line.trim().to_lowercase();
        if answer != "y" && answer != "yes" {
            println!("aborted.");
            return Ok(());
        }
    }

    let report = restore_workspace(&repo, &target_snapshot.tree)?;
    println!(
        "{} workspace to event {}'s pre-state",
        "reverted".green().bold(),
        (&full[..10]).yellow()
    );
    println!(
        "  {} written, {} deleted, {} unchanged",
        report.files_written.to_string().green(),
        report.files_deleted.to_string().red(),
        report.files_unchanged.to_string().bright_black()
    );
    println!();
    println!(
        "{} causari did not move HEAD. Record a new event to mark this revert if you want it in history.",
        "note:".cyan()
    );
    Ok(())
}
