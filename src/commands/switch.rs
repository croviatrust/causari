use anyhow::{Context, Result, anyhow};
use colored::Colorize;

use crate::cli::SwitchArgs;
use crate::repo::Repo;
use crate::snapshot::restore_workspace;
use crate::store::Store;

/// `re switch <session> [--no-sync]`
///
/// The counterpart of `re fork`: moves HEAD to an EXISTING session and (by
/// default) restores the working tree to that session's tip. With `--no-sync`
/// only HEAD moves — useful to adopt a timeline another agent recorded with
/// `--session` without touching the files on disk.
pub fn run(args: SwitchArgs) -> Result<()> {
    let repo = Repo::discover()?;
    let store = Store::new(&repo);

    if !repo.session_ref_path(&args.name)?.exists() {
        let available: Vec<String> = crate::dag::list_sessions(&repo)?
            .into_iter()
            .map(|s| s.name)
            .collect();
        return Err(anyhow!(
            "session '{}' does not exist (available: {})",
            args.name,
            if available.is_empty() {
                "none".to_string()
            } else {
                available.join(", ")
            }
        ));
    }

    if repo.current_session()?.as_deref() == Some(args.name.as_str()) {
        println!("already on session {}", args.name.cyan().bold());
        return Ok(());
    }

    let _lock = repo.lock()?;
    let tip = repo.session_head(&args.name)?;

    // Order matters: validate the target objects and restore the workspace
    // FIRST, move HEAD LAST. If any object is missing or the restore fails,
    // HEAD still names the previous session and the files still belong to
    // it — never a HEAD that disagrees with the working tree.
    let sync_report = if args.no_sync {
        None
    } else {
        match &tip {
            Some(id) => {
                let ev = store.read_event(id).with_context(|| {
                    format!(
                        "session '{}' tip {} is unreadable; HEAD not moved",
                        args.name,
                        &id[..id.len().min(10)]
                    )
                })?;
                let snap = store.read_snapshot(&ev.post_snapshot).with_context(|| {
                    format!(
                        "snapshot of session '{}' tip is unreadable; HEAD not moved",
                        args.name
                    )
                })?;
                // Fail before touching disk if the tree itself is missing.
                store.read_tree(&snap.tree).with_context(|| {
                    format!(
                        "tree of session '{}' tip is unreadable; HEAD not moved",
                        args.name
                    )
                })?;
                Some((id.clone(), restore_workspace(&repo, &snap.tree)?))
            }
            None => None,
        }
    };

    std::fs::write(
        repo.head_path(),
        format!("ref: refs/sessions/{}\n", args.name),
    )?;
    println!(
        "{} session {}",
        "switched to".green().bold(),
        args.name.cyan().bold()
    );

    if args.no_sync {
        println!("  workspace left untouched (--no-sync)");
        return Ok(());
    }

    match sync_report {
        Some((id, report)) => {
            println!(
                "  workspace synced to {}: {} written, {} deleted, {} unchanged",
                (&id[..10]).yellow(),
                report.files_written.to_string().green(),
                report.files_deleted.to_string().red(),
                report.files_unchanged
            );
        }
        None => {
            println!("  session is empty — workspace left as-is");
        }
    }
    Ok(())
}
