use anyhow::Result;
use colored::Colorize;

use crate::banner::print_banner;
use crate::repo::{GitignoreOutcome, Repo};

pub fn run() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let repo = Repo::init(&cwd)?;
    print_banner();
    println!(
        "{} causari repository in {}",
        "Initialized".green().bold(),
        repo.dir.display()
    );

    // Keep the local ledger (captured prompts, completions and reasoning) out
    // of version control by default. Never fail `init` over this — worst case
    // we tell the user to add the entry themselves.
    match repo.ensure_gitignored() {
        Ok(GitignoreOutcome::Created) | Ok(GitignoreOutcome::Appended) => println!(
            "{} added .causari/ to .gitignore \u{2014} captured prompts & reasoning stay out of git",
            "Protected".green().bold()
        ),
        Ok(GitignoreOutcome::AlreadyIgnored) => println!(
            "{} .causari/ is already in .gitignore",
            "Protected".green().bold()
        ),
        Ok(GitignoreOutcome::NotAGitRepo) => println!(
            "{} not a git repo yet \u{2014} keep .causari/ out of version control (it stores captured prompts & reasoning)",
            "Note".yellow().bold()
        ),
        Err(e) => eprintln!(
            "{} could not update .gitignore ({e}) \u{2014} please add .causari/ yourself",
            "Warning".yellow().bold()
        ),
    }

    println!();
    println!("Next steps:");
    println!(
        "  • Zero-cooperation capture:  {} + {}  (proxy + watch)",
        "re proxy".cyan(),
        "re watch".cyan()
    );
    println!(
        "  • Agent integration:       {}  (Claude Code, Cursor, …)",
        "re mcp --install".cyan()
    );
    println!(
        "  • Record an action:        {}",
        "re record -m \"...\"".cyan()
    );
    println!("  • Distill experience:      {}", "re skill distill".cyan());
    println!("  • View history:            {}", "re log".cyan());
    Ok(())
}
