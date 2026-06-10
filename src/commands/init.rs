use anyhow::Result;
use colored::Colorize;

use crate::banner::print_banner;
use crate::repo::Repo;

pub fn run() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let repo = Repo::init(&cwd)?;
    print_banner();
    println!(
        "{} causari repository in {}",
        "Initialized".green().bold(),
        repo.dir.display()
    );
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
