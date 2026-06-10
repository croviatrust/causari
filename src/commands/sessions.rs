use anyhow::Result;
use colored::Colorize;

use crate::dag;
use crate::index;
use crate::repo::Repo;
use crate::store::Store;

/// `re sessions`
///
/// Lists every session branch — the tips of the event DAG. With multiple
/// agents recording concurrently (one session each), this is the fleet
/// overview: who is working, on which timeline, and how far they got.
pub fn run() -> Result<()> {
    let repo = Repo::discover()?;
    let store = Store::new(&repo);

    let sessions = dag::list_sessions(&repo)?;
    if sessions.is_empty() {
        println!(
            "{} no sessions yet (record an event first)",
            "sessions:".yellow().bold()
        );
        return Ok(());
    }

    let current = repo.current_session()?;
    let indexed = index::ensure(&repo, &store)?;

    for s in &sessions {
        let marker = if current.as_deref() == Some(s.name.as_str()) {
            "*".green().bold()
        } else {
            " ".normal()
        };
        match &s.head {
            Some(tip) => {
                // Chain length and last activity, straight from the index.
                let mut count = 0usize;
                let mut cur = Some(tip.clone());
                while let Some(id) = cur {
                    count += 1;
                    cur = indexed.get(&id).and_then(|e| e.parent.clone());
                }
                let last = indexed
                    .get(tip)
                    .map(|e| e.created_at.clone())
                    .unwrap_or_default();
                let agent = indexed
                    .get(tip)
                    .and_then(|e| e.agent.clone())
                    .unwrap_or_else(|| "-".to_string());
                println!(
                    "{} {}  {}  {} event(s)  agent {}  last {}",
                    marker,
                    s.name.cyan().bold(),
                    (&tip[..10]).yellow(),
                    count,
                    agent,
                    last.bright_black()
                );
            }
            None => {
                println!(
                    "{} {}  {}",
                    marker,
                    s.name.cyan().bold(),
                    "(empty)".bright_black()
                );
            }
        }
    }
    Ok(())
}
