use anyhow::{Context, Result};
use colored::Colorize;
use std::collections::HashSet;

use crate::cli::GuardArgs;
use crate::config::GuardConfig;
use crate::repo::Repo;
use crate::store::Store;

struct AlertItem {
    id: String,
    rule: String,
    detail: String,
    agent: Option<String>,
    message: Option<String>,
    severity: Severity,
}

enum Severity {
    Alert,
    Warning,
}

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

    let mut items: Vec<AlertItem> = Vec::new();

    // Built-in rules
    for id in &chain {
        let ev = store.read_event(id)?;
        let writes: HashSet<&str> = ev.writes.iter().map(|s| s.as_str()).collect();
        if writes.is_empty() {
            continue;
        }

        // Rule 1: bulk edit
        if writes.len() > 15 {
            items.push(AlertItem {
                id: id.clone(),
                rule: "bulk edit".into(),
                detail: format!("touched {} files in a single event", writes.len()),
                agent: ev.agent.clone(),
                message: ev.message.clone(),
                severity: Severity::Alert,
            });
        }

        // Rule 2: critical file without test
        let critical_patterns = [
            "auth",
            "login",
            "password",
            "secret",
            "token",
            "credential",
            "payment",
            "billing",
            "crypto",
            "wallet",
            "db",
            "database",
            "migrate",
            "schema",
            "config",
            ".env",
        ];
        let has_critical = writes.iter().any(|w| {
            let low = w.to_lowercase();
            critical_patterns.iter().any(|pat| low.contains(pat))
        });
        let has_test = writes.iter().any(|w| {
            let low = w.to_lowercase();
            low.contains("test") || low.contains("spec") || low.contains("_test.")
        });
        if has_critical && !has_test {
            let crits: Vec<_> = writes
                .iter()
                .filter(|w| {
                    let low = w.to_lowercase();
                    critical_patterns.iter().any(|pat| low.contains(pat))
                })
                .cloned()
                .collect();
            items.push(AlertItem {
                id: id.clone(),
                rule: "critical without test".into(),
                detail: format!("modified {} but no test file touched", crits.join(", ")),
                agent: ev.agent.clone(),
                message: ev.message.clone(),
                severity: Severity::Alert,
            });
        }

        // Rule 3: source edit but zero tests
        let has_source = writes.iter().any(|w| {
            w.ends_with(".rs")
                || w.ends_with(".ts")
                || w.ends_with(".js")
                || w.ends_with(".py")
                || w.ends_with(".go")
        });
        if has_source && !has_test {
            items.push(AlertItem {
                id: id.clone(),
                rule: "missing tests".into(),
                detail: "modified source files but no tests".into(),
                agent: ev.agent.clone(),
                message: ev.message.clone(),
                severity: Severity::Warning,
            });
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
                    items.push(AlertItem {
                        id: id.clone(),
                        rule: rule.name.clone(),
                        detail: format!(
                            "matched '{}' in {} files (threshold: {})",
                            rule.when,
                            writes.len(),
                            threshold
                        ),
                        agent: ev.agent.clone(),
                        message: ev.message.clone(),
                        severity: Severity::Alert,
                    });
                }
            }
        }
    }

    let alerts = items
        .iter()
        .filter(|i| matches!(i.severity, Severity::Alert))
        .count();
    let warnings = items
        .iter()
        .filter(|i| matches!(i.severity, Severity::Warning))
        .count();

    if args.badge {
        let badge_path = repo.root.join(".causari").join("guard-badge.svg");
        let svg = generate_badge(alerts, warnings);
        std::fs::write(&badge_path, svg)
            .with_context(|| format!("writing {}", badge_path.display()))?;
        println!(
            "{} badge written to {}",
            "✓".green().bold(),
            badge_path.display()
        );
        return Ok(());
    }

    if args.summary {
        print_summary(&items, alerts, warnings);
        return Ok(());
    }

    // Default terminal output
    println!(
        "{} scanning last {} events for risky patterns…",
        "causari guard:".green().bold(),
        limit
    );

    for item in &items {
        let short = &item.id[..10];
        let sev_icon = match item.severity {
            Severity::Alert => "▲".red().bold().to_string(),
            Severity::Warning => "△".yellow().bold().to_string(),
        };
        println!(
            "  {} {} {} {}  {}",
            sev_icon,
            short.bright_black(),
            item.rule.yellow().bold(),
            "—".bright_black(),
            &item.detail
        );
        if let Some(agent) = &item.agent {
            println!("    agent: {}", agent.cyan());
        }
        if let Some(msg) = &item.message {
            println!("    msg:   {}", msg);
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

fn generate_badge(alerts: usize, warnings: usize) -> String {
    let (color, text) = if alerts > 0 {
        ("#EF4444", format!("guard: {} alerts", alerts))
    } else if warnings > 0 {
        ("#F59E0B", format!("guard: {} warnings", warnings))
    } else {
        ("#22C55E", "guard: passing".into())
    };
    let width = 140 + text.len() * 7;
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="28" role="img" aria-label="Causari Guard: {text}">
  <title>Causari Guard: {text}</title>
  <g shape-rendering="crispEdges">
    <rect width="105" height="28" fill="#0B1437"/>
    <rect x="105" width="{badge_width}" height="28" fill="{color}"/>
  </g>
  <g fill="#fff" text-anchor="middle" font-family="Verdana,Geneva,DejaVu Sans,sans-serif" font-size="12">
    <text x="52.5" y="18">causari</text>
    <text x="{text_x}" y="18">{text}</text>
  </g>
</svg>"##,
        width = width,
        badge_width = width - 105,
        text = text,
        text_x = 105 + (width - 105) / 2,
        color = color,
    )
}

fn print_summary(items: &[AlertItem], alerts: usize, warnings: usize) {
    let status = if alerts > 0 {
        "❌ failing"
    } else if warnings > 0 {
        "⚠️ warnings"
    } else {
        "✅ passing"
    };
    println!("## Causari Guard — {}", status);
    println!();
    println!("| Event | Agent | Rule | Detail |");
    println!("|---|---|---|---|");
    for item in items {
        let short = &item.id[..10];
        let agent = item.agent.as_deref().unwrap_or("—");
        let sev = match item.severity {
            Severity::Alert => "🔴",
            Severity::Warning => "🟡",
        };
        println!(
            "| `{}` | {} | {} {} | {} |",
            short, agent, sev, item.rule, item.detail
        );
    }
    if items.is_empty() {
        println!("| — | — | — | No risky patterns found |");
    }
    println!();
    println!("<sub>Powered by [Causari](https://causari.dev)</sub>");
}
