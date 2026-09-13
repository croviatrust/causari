use anyhow::{Result, anyhow};
use colored::Colorize;

use crate::cli::ForkArgs;
use crate::object::resolve_id;
use crate::repo::Repo;
use crate::snapshot::restore_workspace;
use crate::store::Store;

/// `re fork <branch-name> [--from <event-id>]`
///
/// Creates a new session branch pointing at the given event (or HEAD by default)
/// and switches HEAD to it. The working tree is restored to that event's
/// post-state. From here, new `re record` calls extend the new timeline,
/// leaving the original branch intact.
///
/// This is what enables multiverse exploration: same starting point, different
/// agent or different prompt, two timelines you can later diff.
pub fn run(args: ForkArgs) -> Result<()> {
    let repo = Repo::discover()?;
    let store = Store::new(&repo);

    let new_ref = repo.session_ref_path(&args.name)?;

    let from_id = match args.from {
        Some(s) => resolve_id(&repo.objects_dir(), &s)?,
        None => repo
            .head_event()?
            .ok_or_else(|| anyhow!("no HEAD yet; record an event before forking"))?,
    };

    if new_ref.exists() {
        return Err(anyhow!("branch '{}' already exists", args.name));
    }

    // Validate every object and restore the workspace FIRST; create the ref
    // and move HEAD only once the working tree really is at `from_id`.
    let _lock = repo.lock()?;
    let ev = store.read_event(&from_id)?;
    let snap = store.read_snapshot(&ev.post_snapshot)?;
    store.read_tree(&snap.tree)?;
    let report = restore_workspace(&repo, &snap.tree)?;

    if let Some(parent) = new_ref.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&new_ref, format!("{}\n", from_id))?;
    std::fs::write(
        repo.head_path(),
        format!("ref: refs/sessions/{}\n", args.name),
    )?;

    println!(
        "{} branch {} from event {}",
        "forked".green().bold(),
        args.name.cyan(),
        (&from_id[..10]).yellow()
    );
    println!(
        "  workspace synced: {} written, {} deleted",
        report.files_written.to_string().green(),
        report.files_deleted.to_string().red()
    );
    println!();
    println!(
        "  {} record events here freely — original branch untouched.",
        "tip:".bright_black()
    );
    Ok(())
}
