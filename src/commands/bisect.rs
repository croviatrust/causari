use anyhow::{Context, Result, anyhow};
use colored::Colorize;
use std::process::Command;

use crate::cli::BisectArgs;
use crate::object::resolve_id;
use crate::repo::Repo;
use crate::snapshot::{plan_restore, restore_workspace, snapshot_workspace};
use crate::store::Store;

/// `re bisect --good <id> --bad <id> --test "<cmd>"`
///
/// Binary-search the event range to find the agent action that broke the test
/// command. Causari restores each candidate state, executes the test, and uses
/// the exit code to narrow the search.
///
/// The signature: at the end we point to the FIRST BAD event — the one whose
/// post-state fails the test, while its parent's post-state still passes.
pub fn run(args: BisectArgs) -> Result<()> {
    let repo = Repo::discover()?;
    let store = Store::new(&repo);
    let original_tree = snapshot_workspace(&repo)?;
    plan_restore(&repo, &original_tree)?;
    let result = search(&repo, &store, &args);
    // Restore the actual starting workspace, including unrecorded edits, on
    // both success and ordinary errors. Shell commands are not sandboxed.
    let recovery = restore_workspace(&repo, &original_tree);
    match (result, recovery) {
        (Ok(()), Ok(_)) => {
            println!("workspace restored to its pre-bisect state (including unrecorded files).");
            Ok(())
        }
        (Err(error), Ok(_)) => Err(error),
        (Ok(()), Err(error)) => Err(error).context(format!(
            "bisect finished but workspace recovery failed; recovery tree: {original_tree}"
        )),
        (Err(error), Err(recovery)) => Err(anyhow!(
            "bisect failed: {error:#}; workspace recovery also failed: {recovery:#}; recovery tree: {original_tree}"
        )),
    }
}

fn search(repo: &Repo, store: &Store, args: &BisectArgs) -> Result<()> {
    let good_id = resolve_id(&repo.objects_dir(), &args.good)?;
    let bad_id = resolve_id(&repo.objects_dir(), &args.bad)?;

    // Build the chain from `bad` walking back to `good`. We need linear ancestry.
    let chain = ancestry_between(store, &good_id, &bad_id)?;
    if chain.is_empty() {
        return Err(anyhow!(
            "good event {} is not an ancestor of bad event {}",
            &good_id[..10],
            &bad_id[..10]
        ));
    }

    println!(
        "{} bisecting {} events between {} (good) and {} (bad)",
        "causari:".green().bold(),
        chain.len(),
        (&good_id[..10]).green(),
        (&bad_id[..10]).red()
    );
    println!("  test command: {}", args.test.cyan());
    println!();

    if !test_passes(repo, store, &good_id, &args.test)? {
        return Err(anyhow!(
            "known-good event fails the test; refusing an invalid bisect range"
        ));
    }
    if test_passes(repo, store, &bad_id, &args.test)? {
        return Err(anyhow!(
            "known-bad event passes the test; refusing an invalid bisect range"
        ));
    }

    // chain[0] = first event after good, chain[last] = bad.
    // Invariant: passes(chain[-1]) might be false, passes(good) is true.
    let mut lo: usize = 0;
    let mut hi: usize = chain.len(); // exclusive upper bound for "still passing"
    // We search for the largest i such that test passes at chain[i-1] / fails at chain[i].

    // Binary search.
    let mut first_bad: Option<usize> = None;
    let mut step = 0usize;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let candidate = &chain[mid];
        step += 1;
        println!(
            "  {} step {} — testing {}",
            "→".cyan(),
            step,
            (&candidate[..10]).yellow()
        );
        if test_passes(repo, store, candidate, &args.test)? {
            println!("    {} passes", "✓".green());
            lo = mid + 1;
        } else {
            println!("    {} fails", "✗".red());
            first_bad = Some(mid);
            hi = mid;
        }
    }

    println!();
    match first_bad {
        Some(idx) => {
            let culprit = &chain[idx];
            let ev = store.read_event(culprit)?;
            println!(
                "{} first bad event: {}",
                "found:".red().bold(),
                culprit.yellow()
            );
            if let Some(a) = &ev.agent {
                println!("  agent:   {}", a);
            }
            if let Some(t) = &ev.tool {
                println!("  tool:    {}", t);
            }
            if let Some(m) = &ev.message {
                println!("  message: {}", m);
            }
            if let Some(p) = &ev.prompt {
                println!("  prompt:  {}", p);
            }
            println!();
            println!(
                "  inspect: {}",
                format!("re show {}", &culprit[..10]).cyan()
            );
            println!(
                "  revert:  {}",
                format!("re revert {}", &culprit[..10]).cyan()
            );
        }
        None => {
            println!(
                "{} all events in range pass the test. The breakage is outside the bisect range.",
                "no culprit:".yellow().bold()
            );
        }
    }

    Ok(())
}

/// Returns the chain of event ids from (good, exclusive) up to (bad, inclusive).
/// Result is ordered oldest-first.
fn ancestry_between(store: &Store, good: &str, bad: &str) -> Result<Vec<String>> {
    let mut chain: Vec<String> = Vec::new();
    let mut cur = Some(bad.to_string());
    while let Some(id) = cur {
        if id == good {
            chain.reverse();
            return Ok(chain);
        }
        chain.push(id.clone());
        let ev = store.read_event(&id)?;
        cur = ev.parent;
    }
    Err(anyhow!(
        "good {} not found in ancestry of bad {}",
        &good[..10],
        &bad[..10]
    ))
}

fn test_passes(repo: &Repo, store: &Store, event_id: &str, test_cmd: &str) -> Result<bool> {
    let ev = store.read_event(event_id)?;
    let snap = store.read_snapshot(&ev.post_snapshot)?;
    restore_workspace(repo, &snap.tree)?;

    // Run the command from the repo root. We delegate to the system shell so
    // users can pass pipelines like "npm test && npm run lint".
    let (program, args_slice): (&str, Vec<&str>) = if cfg!(target_os = "windows") {
        ("cmd", vec!["/C", test_cmd])
    } else {
        ("sh", vec!["-c", test_cmd])
    };

    let status = Command::new(program)
        .args(args_slice)
        .current_dir(&repo.root)
        .status()
        .with_context(|| format!("running test command: {}", test_cmd))?;
    match status.code() {
        Some(0) => Ok(true),
        Some(1..=124) => Ok(false),
        Some(code) => Err(anyhow!(
            "test command aborted with exit code {code}; not classifying it as a regression (125/126/127 are skip or execution errors)"
        )),
        None => Err(anyhow!(
            "test command terminated by signal; not classifying it as a regression"
        )),
    }
}
