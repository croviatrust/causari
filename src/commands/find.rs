use anyhow::Result;
use colored::Colorize;

use crate::cli::FindArgs;
use crate::repo::Repo;
use crate::store::Store;

/// `re find <query>`
///
/// Text search across the prompt, message, reasoning and tool of every event.
/// Returns the most relevant events first, scored by simple term-frequency
/// across the queried fields. This is the bridge between "I remember an agent
/// did something about X" and the actual event id.
///
/// (Embeddings-based semantic search is a later upgrade — this baseline is
/// already useful and has zero external dependencies.)
pub fn run(args: FindArgs) -> Result<()> {
    let repo = Repo::discover()?;
    let store = Store::new(&repo);

    let head = repo.head_event()?;
    let mut cur = head;
    let query_terms: Vec<String> = args
        .query
        .split_whitespace()
        .map(|t| t.to_lowercase())
        .collect();

    let mut hits: Vec<(usize, String, String)> = Vec::new();

    while let Some(id) = cur {
        let ev = store.read_event(&id)?;
        let haystack = format!(
            "{} {} {} {}",
            ev.message.clone().unwrap_or_default(),
            ev.prompt.clone().unwrap_or_default(),
            ev.reasoning.clone().unwrap_or_default(),
            ev.tool.clone().unwrap_or_default()
        )
        .to_lowercase();

        let score: usize = query_terms
            .iter()
            .map(|t| haystack.matches(t.as_str()).count())
            .sum();
        if score > 0 {
            let preview = ev
                .message
                .clone()
                .or(ev.prompt.clone())
                .unwrap_or_else(|| "(no message)".to_string());
            hits.push((score, id.clone(), preview));
        }
        cur = ev.parent;
    }

    if hits.is_empty() {
        println!(
            "{} no events match {:?}",
            "no results:".yellow().bold(),
            args.query
        );
        return Ok(());
    }

    hits.sort_by_key(|h| std::cmp::Reverse(h.0));
    let limit = args.limit.unwrap_or(10);
    println!(
        "{} {} match(es) for {:?}",
        "found:".green().bold(),
        hits.len(),
        args.query
    );
    for (score, id, preview) in hits.iter().take(limit) {
        let short = &id[..10];
        let trimmed = if preview.len() > 80 {
            format!("{}…", &preview[..80])
        } else {
            preview.clone()
        };
        println!(
            "  {} {}  {}  {}",
            format!("[{}]", score).bright_black(),
            short.yellow(),
            "·".bright_black(),
            trimmed
        );
    }
    Ok(())
}
