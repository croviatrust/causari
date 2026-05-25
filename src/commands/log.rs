use anyhow::Result;
use colored::Colorize;

use crate::cli::LogArgs;
use crate::repo::Repo;
use crate::store::Store;

pub fn run(args: LogArgs) -> Result<()> {
    let repo = Repo::discover()?;
    let store = Store::new(&repo);

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
