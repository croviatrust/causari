use anyhow::Result;
use colored::Colorize;
use std::collections::HashSet;

use crate::cli::GuardArgs;
use crate::config::GuardConfig;
use crate::repo::Repo;
use crate::store::Store;

/// `re guard` — causal watchdog.
///
/// Scans the event ledger for risky patterns that no other tool can detect.
/// Rules are hard-coded + user-defined in `.causari/guard.toml`.
/// All analysis is local; no data leaves the machine.
pub fn run(args: GuardArgs) -> Result<()> {
    let repo = Repo::discover()?;
    let store = Store::new(&repo);
    let cfg = GuardConfig::load(&repo.root)?;

    let head = repo.head_event()?;
    let limit = args.limit.unwrap_or(20);

    println!(
        "{} scanning last {} events for risky patterns…",
        "causari guard:".green().bold(),
        limit
    );

    let mut chain: Vec<String> = Vec::new();
    let mut cur = head;
    while let Some(id) = cur {
        let ev = store.read_event(&id)?;
        chain.push(id.clone());
        if chain.len() >= limit {
            break;
        }
        cur = ev.parent;
    }
    chain.reverse();

    let mut alerts = 0usize;
    let mut warnings = 0usize;

    // Built-in rules
    for id in &chain {
        let ev = store.read_event(id)?;
        let writes: HashSet<&str> = ev.writes.iter().map(|s| s.as_str()).collect();
        if writes.is_empty() {
            continue;
        }

        // Rule 1: bulk edit
        if writes.len() > 15 {
            print_alert(id, &ev, "bulk edit", &format!(
                "touched {} files in a single event — easy to miss side-effects",
                writes.len()
            ));
            alerts += 1;
        }

        // Rule 2: critical file without test
        let critical_patterns = ["auth", "login", "password", "secret", "token", "credential",
                                 "payment", "billing", "crypto", "wallet", "db", "database",
                                 "migrate", "schema", "config", ".env"];
        let has_critical = writes.iter().any(|w| {
            let low = w.to_lowercase();
            critical_patterns.iter().any(|pat| low.contains(pat))
        });
        let has_test = writes.iter().any(|w| {
            let low = w.to_lowercase();
            low.contains("test") || low.contains("spec") || low.contains("_test.")
        });
        if has_critical && !has_test {
            let crits: Vec<_> = writes.iter().filter(|w| {
                let low = w.to_lowercase();
                critical_patterns.iter().any(|pat| low.contains(pat))
            }).cloned().collect();
            print_alert(id, &ev, "critical without test", &format!(
                "modified {} but no test file was touched",
                crits.join(", ")
            ));
            alerts += 1;
        }

        // Rule 3: source edit but zero tests
        let has_source = writes.iter().any(|w| {
            w.ends_with(".rs") || w.ends_with(".ts") || w.ends_with(".js")
                || w.ends_with(".py") || w.ends_with(".go")
        });
        if has_source && !has_test {
            print_alert(id, &ev, "missing tests", "modified source files but no tests");
            warnings += 1;
        }
    }

    // User-defined rules from .causari/guard.toml
    for rule in &cfg.rules {
        for id in &chain {
            let ev = store.read_event(id)?;
            let writes: HashSet<&str> = ev.writes.iter().map(|s| s.as_str()).collect();
            if writes.is_empty() {
                continue;
            }
            let low_when = rule.when.to_lowercase();
            let matched = writes.iter().any(|w| w.to_lowercase().contains(&low_when));
            if matched {
                let threshold = rule.threshold.unwrap_or(1);
                if writes.len() >= threshold {
                    print_alert(id, &ev, &rule.name, &format!(
                        "matched '{}' in {} files (threshold: {})",
                        rule.when,
                        writes.len(),
                        threshold
                    ));
                    alerts += 1;
                }
            }
        }
    }

    println!();
    if alerts == 0 && warnings == 0 {
        println!("{} no risky patterns found.", "✓".green().bold());
    } else {
        println!(
            "{} {} alert(s), {} warning(s) found.",
            "!".red().bold(),
            alerts,
            warnings
        );
        println!("  Review with:  re show <id>   re diff <id>   re trace <file>:<line>");
    }

    Ok(())
}

fn print_alert(id: &str, ev: &crate::object::Event, rule: &str, detail: &str) {
    let short = &id[..10];
    println!(
        "  {} {} {} {}  {}",
        "▲".red().bold(),
        short.bright_black(),
        rule.yellow().bold(),
        "—".bright_black(),
        detail
    );
    if let Some(agent) = &ev.agent {
        println!("    agent: {}", agent.cyan());
    }
    if let Some(msg) = &ev.message {
        println!("    msg:   {}", msg);
    }
}
