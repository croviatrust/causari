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
/// Runs on the metadata index (one JSONL read) instead of opening every
/// object file, and searches ALL sessions, not just the current one.
///
/// (Embeddings-based semantic search is a later upgrade — this baseline is
/// already useful and has zero external dependencies.)
pub fn run(args: FindArgs) -> Result<()> {
    let repo = Repo::discover()?;
    let store = Store::new(&repo);

    let query_terms: Vec<String> = args
        .query
        .split_whitespace()
        .map(|t| t.to_lowercase())
        .collect();

    let indexed = crate::index::ensure(&repo, &store)?;
    // (score, created_at, id, preview) — created_at breaks score ties so
    // results are deterministic and newest-first.
    let mut hits: Vec<(usize, String, String, String)> = Vec::new();

    for (id, entry) in &indexed {
        let haystack = format!(
            "{} {} {} {}",
            entry.message.clone().unwrap_or_default(),
            entry.prompt.clone().unwrap_or_default(),
            entry.reasoning.clone().unwrap_or_default(),
            entry.tool.clone().unwrap_or_default()
        )
        .to_lowercase();

        let score: usize = query_terms
            .iter()
            .map(|t| haystack.matches(t.as_str()).count())
            .sum();
        if score > 0 {
            let preview = entry
                .message
                .clone()
                .or(entry.prompt.clone())
                .unwrap_or_else(|| "(no message)".to_string());
            hits.push((score, entry.created_at.clone(), id.clone(), preview));
        }
    }

    if hits.is_empty() {
        println!(
            "{} no events match {:?}",
            "no results:".yellow().bold(),
            args.query
        );
        return Ok(());
    }

    hits.sort_by(|a, b| (b.0, &b.1).cmp(&(a.0, &a.1)));
    let limit = args.limit.unwrap_or(10);
    println!(
        "{} {} match(es) for {:?}",
        "found:".green().bold(),
        hits.len(),
        args.query
    );
    for (score, _ts, id, preview) in hits.iter().take(limit) {
        let short = &id[..10];
        let trimmed = if preview.chars().count() > 80 {
            let cut: String = preview.chars().take(80).collect();
            format!("{}…", cut)
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
