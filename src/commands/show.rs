use anyhow::Result;
use colored::Colorize;

use crate::cli::ShowArgs;
use crate::object::resolve_id;
use crate::repo::Repo;
use crate::store::Store;

pub fn run(args: ShowArgs) -> Result<()> {
    let repo = Repo::discover()?;
    let store = Store::new(&repo);
    let full = resolve_id(&repo.objects_dir(), &args.id)?;
    let ev = store.read_event(&full)?;

    println!("{} {}", "event".yellow().bold(), full.yellow());
    if let Some(p) = &ev.parent {
        println!("  parent:  {}", &p[..10]);
    }
    if let Some(a) = &ev.agent {
        println!("  agent:   {}", a);
    }
    if let Some(t) = &ev.tool {
        println!("  tool:    {}", t.cyan());
    }
    println!("  date:    {}", ev.created_at);
    println!("  pre:     {}", &ev.pre_snapshot[..10]);
    println!("  post:    {}", &ev.post_snapshot[..10]);
    if let Some(c) = ev.exit_code {
        println!("  exit:    {}", c);
    }
    if let Some(m) = &ev.message {
        println!();
        println!("  {}", m);
    }
    Ok(())
}
