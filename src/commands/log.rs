use anyhow::Result;
use colored::Colorize;

use crate::cli::LogArgs;
use crate::dag;
use crate::repo::Repo;
use crate::store::Store;

pub fn run(args: LogArgs) -> Result<()> {
    let repo = Repo::discover()?;
    let store = Store::new(&repo);

    if args.all {
        return run_all(&repo, &store, &args);
    }

    let mut current = repo.head_event()?;
    if current.is_none() {
        println!("{}", "no events recorded yet".bright_black());
        return Ok(());
    }

    let mut shown = 0usize;
    while let Some(id) = current {
        if shown >= args.limit {
            break;
        }
        let ev = store.read_event(&id)?;
        let short = &id[..10];

        if args.oneline {
            println!(
                "{}  {}  {}",
                short.yellow(),
                ev.tool.as_deref().unwrap_or("-").cyan(),
                ev.message.as_deref().unwrap_or("")
            );
        } else {
            println!("{} {}", "event".yellow().bold(), short.yellow());
            if let Some(a) = &ev.agent {
                println!("  agent:   {}", a);
            }
            if let Some(t) = &ev.tool {
                println!("  tool:    {}", t.cyan());
            }
            println!("  date:    {}", ev.created_at);
            if let Some(m) = &ev.message {
                println!();
                println!("    {}", m);
            }
            println!();
        }

        current = ev.parent;
        shown += 1;
    }
    Ok(())
}

/// `re log --all` — the DAG view: every event from every session, newest
/// first, with session-tip and fork-point markers. This is how you read a
/// multi-agent timeline at a glance.
fn run_all(repo: &Repo, store: &Store, args: &LogArgs) -> Result<()> {
    let (events, info) = dag::walk_all(repo, store)?;
    if events.is_empty() {
        println!("{}", "no events recorded yet".bright_black());
        return Ok(());
    }

    for (id, ev) in events.iter().take(args.limit) {
        let short = &id[..10];
        let mut markers = String::new();
        if let Some(session) = info.tips.get(id) {
            markers.push_str(&format!(" {}", format!("[{}]", session).cyan().bold()));
        }
        if info.is_fork_point(id) {
            markers.push_str(&format!(" {}", "⑂ fork".magenta()));
        }

        if args.oneline {
            println!(
                "{}  {}  {}{}",
                short.yellow(),
                ev.tool.as_deref().unwrap_or("-").cyan(),
                ev.message.as_deref().unwrap_or(""),
                markers
            );
        } else {
            println!("{} {}{}", "event".yellow().bold(), short.yellow(), markers);
            if let Some(a) = &ev.agent {
                println!("  agent:   {}", a);
            }
            if let Some(t) = &ev.tool {
                println!("  tool:    {}", t.cyan());
            }
            println!("  date:    {}", ev.created_at);
            if let Some(m) = &ev.message {
                println!();
                println!("    {}", m);
            }
            println!();
        }
    }
    Ok(())
}
