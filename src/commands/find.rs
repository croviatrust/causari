use anyhow::Result;
use colored::Colorize;

use crate::cli::FindArgs;
use crate::repo::Repo;
use crate::skill::{self, Trust};
use crate::store::Store;

/// `re find <query>`
///
/// Search signed skills first (trust-ranked), then raw ledger events.
/// Skills are distilled experience; events are the raw record. Both are
/// searched across every session via the metadata index / skill library.
pub fn run(args: FindArgs) -> Result<()> {
    let repo = Repo::discover()?;
    let store = Store::new(&repo);
    let limit = args.limit.unwrap_or(10);

    let query_terms: Vec<String> = args
        .query
        .split_whitespace()
        .map(|t| t.to_lowercase())
        .collect();

    // 1. Signed skills — proven experience outranks raw events.
    let skills = skill::load_skills(&repo)?;
    let mut skill_hits: Vec<(usize, String, skill::SkillEnvelope)> = skills
        .into_iter()
        .filter(|(_, env)| skill::verify_envelope(env).is_ok())
        .map(|(id, env)| (skill::score_skill(&env, &query_terms), id, env))
        .filter(|(score, _, _)| *score > 0)
        .collect();
    skill_hits.sort_by_key(|h| std::cmp::Reverse(h.0));

    // 2. Raw events from the index (all sessions, one read).
    let indexed = crate::index::ensure(&repo, &store)?;
    let mut event_hits: Vec<(usize, String, String, String)> = Vec::new();
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
            event_hits.push((score, entry.created_at.clone(), id.clone(), preview));
        }
    }
    event_hits.sort_by(|a, b| (b.0, &b.1).cmp(&(a.0, &a.1)));

    if skill_hits.is_empty() && event_hits.is_empty() {
        println!(
            "{} no skills or events match {:?}",
            "no results:".yellow().bold(),
            args.query
        );
        return Ok(());
    }

    let total = skill_hits.len() + event_hits.len();
    println!(
        "{} {} match(es) for {:?}",
        "found:".green().bold(),
        total,
        args.query
    );

    let mut shown = 0usize;
    for (score, id, env) in &skill_hits {
        if shown >= limit {
            break;
        }
        let trust = match env.trust() {
            Trust::Recorded => format!("{} recorded", env.trust().badge()).bright_black(),
            Trust::Verified => format!("{} verified", env.trust().badge()).green(),
            Trust::Proven => format!("{} proven", env.trust().badge()).yellow().bold(),
        };
        let preview = if env.skill.title.chars().count() > 80 {
            let cut: String = env.skill.title.chars().take(80).collect();
            format!("{}…", cut)
        } else {
            env.skill.title.clone()
        };
        println!(
            "  {} {} {}  {}  {}",
            format!("[{}]", score).bright_black(),
            trust,
            (&id[..10]).yellow(),
            "·".bright_black(),
            preview
        );
        shown += 1;
    }

    for (score, _ts, id, preview) in &event_hits {
        if shown >= limit {
            break;
        }
        let trimmed = if preview.chars().count() > 80 {
            let cut: String = preview.chars().take(80).collect();
            format!("{}…", cut)
        } else {
            preview.clone()
        };
        println!(
            "  {} {} {}  {}  {}",
            format!("[{}]", score).bright_black(),
            "event".bright_black(),
            (&id[..10]).yellow(),
            "·".bright_black(),
            trimmed
        );
        shown += 1;
    }
    Ok(())
}
