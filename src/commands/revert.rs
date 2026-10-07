use anyhow::Result;
use colored::Colorize;
use std::io::{BufRead, Write};

use crate::cli::RevertArgs;
use crate::commands::impact::compute_impact;
use crate::object::resolve_id;
use crate::repo::Repo;
use crate::snapshot::{plan_restore, restore_workspace};
use crate::store::Store;

pub fn run(args: RevertArgs) -> Result<()> {
    let repo = Repo::discover()?;
    let store = Store::new(&repo);
    let full = resolve_id(&repo.objects_dir(), &args.id)?;
    let ev = store.read_event(&full)?;
    let target_snapshot = store.read_snapshot(&ev.pre_snapshot)?;

    // Preview of the forward ledger graph before restoring the pre-snapshot.
    // These events read files this one wrote. Restoring the tree does not
    // establish that they would not have happened.
    let impacted = compute_impact(&repo, &store, &full)?;
    if !impacted.is_empty() {
        println!(
            "{} restoring before {} touches files read by {} later ledger event(s):",
            "ledger preview:".magenta().bold(),
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
            "   {} those events read files this one wrote. Restoring the tree does not rewrite them.",
            "note:".bright_black()
        );
        println!();
    }

    let plan = plan_restore(&repo, &target_snapshot.tree)?;
    println!(
        "restore plan: {} written, {} deleted, {} unchanged",
        plan.files_written, plan.files_deleted, plan.files_unchanged
    );
    if args.dry_run {
        println!("dry run: snapshot verified; no workspace files changed.");
        return Ok(());
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

    // Record the revert as an event of its own. Without it the next
    // recorder's pre-state would still be the old tip and the revert's
    // changes would be attributed to whatever agent acts next (review
    // finding B4: `re why` answered "claude" for a line a human revert
    // restored).
    let id = record_revert(&repo, &store, &full, &target_snapshot.tree)?;
    println!(
        "  {} recorded as {} (tool: revert, evidence: declared)",
        "history:".cyan(),
        (&id[..10]).yellow()
    );
    Ok(())
}

fn record_revert(
    repo: &Repo,
    store: &Store,
    reverted: &str,
    restored_tree: &str,
) -> Result<String> {
    let _lock = repo.lock()?;
    let parent = crate::commit::resolve_parent(repo, None)?;
    let pre = crate::commit::resolve_pre_snapshot(repo, store, &parent)?;
    let post = store.write_snapshot(&crate::object::Snapshot {
        tree: restored_tree.to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
    })?;
    let writes: Vec<String> = crate::snapshot::effective_writes(store, &pre, &post)?
        .into_iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    let ev = crate::object::Event {
        schema: "causari.event.v0.2".to_string(),
        parent,
        agent: Some("human".to_string()),
        model: None,
        tool: Some("revert".to_string()),
        message: Some(format!("revert to pre-state of {}", &reverted[..10])),
        prompt: None,
        reasoning: None,
        reads: Vec::new(),
        writes,
        tokens_in: None,
        tokens_out: None,
        cost_usd: None,
        pre_snapshot: pre,
        post_snapshot: post,
        exit_code: Some(0),
        created_at: chrono::Utc::now().to_rfc3339(),
        evidence: Some(crate::object::Evidence::declared("re revert")),
        redactions: 0,
    };
    crate::commit::commit_event(repo, store, &ev, None)
}
