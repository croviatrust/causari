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
            "{} added {} to .gitignore \u{2014} captured prompts & reasoning stay out of git",
            "Protected".green().bold(),
            ".causari/"
        ),
        Ok(GitignoreOutcome::AlreadyIgnored) => println!(
            "{} {} is already in .gitignore",
            "Protected".green().bold(),
            ".causari/"
        ),
        Ok(GitignoreOutcome::NotAGitRepo) => println!(
            "{} not a git repo yet \u{2014} keep {} out of version control (it stores captured prompts & reasoning)",
            "Note".yellow().bold(),
            ".causari/"
        ),
        Err(e) => eprintln!(
            "{} could not update .gitignore ({e}) \u{2014} please add {} yourself",
            "Warning".yellow().bold(),
            ".causari/"
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
