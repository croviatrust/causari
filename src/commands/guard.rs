use anyhow::{Context, Result};
use colored::Colorize;
use std::collections::HashSet;
use std::process::Command;

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

/// A unit of change to be analyzed, sourced either from a Causari event
/// ledger or from the git commit history (fallback when no `.causari` exists).
struct ChangeSet {
    id: String,
    agent: Option<String>,
    message: Option<String>,
    writes: Vec<String>,
}

/// Source of the change history that was scanned.
enum Source {
    Causari,
    Git,
}

/// `re guard` — causal watchdog.
///
/// Scans the event ledger for risky patterns that no other tool can detect.
/// Rules are hard-coded + user-defined in `.causari/guard.toml`.
/// All analysis is local; no data leaves the machine.
pub fn run(args: GuardArgs) -> Result<()> {
    let limit = args.limit.unwrap_or(20);

    // Prefer the Causari event ledger; fall back to git history so the
    // watchdog works on any repository (e.g. inside CI on a fresh checkout).
    let (changes, cfg, source) = match Repo::discover() {
        Ok(repo) => {
            let store = Store::new(&repo);
            let cfg = GuardConfig::load(&repo.root)?;
            let changes = collect_from_causari(&repo, &store, limit)?;
            (changes, cfg, Source::Causari)
        }
        Err(_) => {
            let changes = collect_from_git(limit)
                .context("no Causari ledger found and unable to read git history")?;
            (changes, GuardConfig::default(), Source::Git)
        }
    };

    let items = apply_rules(&changes, &cfg);

    let alerts = items
        .iter()
        .filter(|i| matches!(i.severity, Severity::Alert))
        .count();
    let warnings = items
        .iter()
        .filter(|i| matches!(i.severity, Severity::Warning))
        .count();

    if args.badge {
        let badge_dir = std::path::Path::new(".causari");
        let badge_path = if badge_dir.is_dir() {
            badge_dir.join("guard-badge.svg")
        } else {
            std::path::PathBuf::from("guard-badge.svg")
        };
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
        print_summary(&items, alerts, warnings, &source);
        return Ok(());
    }

    // Default terminal output
    let unit = match source {
        Source::Causari => "events",
        Source::Git => "commits",
    };
    println!(
        "{} scanning last {} {} for risky patterns…",
        "causari guard:".green().bold(),
        changes.len(),
        unit
    );

    for item in &items {
        let short = short_id(&item.id);
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
            item.detail
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
        match source {
            Source::Causari => {
                println!("  Review with:  re show <id>   re diff <id>   re trace <file>:<line>");
            }
            Source::Git => {
                println!("  Review with:  git show <sha>");
            }
        }
    }

    Ok(())
}

/// Truncate an id for display (handles both long causari ids and short git SHAs).
fn short_id(id: &str) -> &str {
    let n = id.len().min(10);
    &id[..n]
}

/// Walk the Causari event ledger from HEAD back `limit` events.
fn collect_from_causari(repo: &Repo, store: &Store, limit: usize) -> Result<Vec<ChangeSet>> {
    let mut chain: Vec<ChangeSet> = Vec::new();
    let mut cur = repo.head_event()?;
    while let Some(id) = cur {
        let ev = store.read_event(&id)?;
        chain.push(ChangeSet {
            id: id.clone(),
            agent: ev.agent.clone(),
            message: ev.message.clone(),
            writes: ev.writes.clone(),
        });
        if chain.len() >= limit {
            break;
        }
        cur = ev.parent;
    }
    chain.reverse();
    Ok(chain)
}

/// Read the last `limit` commits from git, with their changed files.
/// Each commit becomes one ChangeSet: author -> agent, subject -> message.
fn collect_from_git(limit: usize) -> Result<Vec<ChangeSet>> {
    // Records separated by NUL; fields: hash, author, subject, then file list.
    let out = Command::new("git")
        .args([
            "log",
            &format!("-n{}", limit),
            "--no-merges",
            "--name-only",
            "--pretty=format:%x00%H%x1f%an%x1f%s",
        ])
        .output()
        .context("failed to run `git log` (is git installed and is this a git repo?)")?;

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(anyhow::anyhow!("git log failed: {}", err.trim()));
    }

    let text = String::from_utf8_lossy(&out.stdout);
    let mut sets: Vec<ChangeSet> = Vec::new();

    for record in text.split('\u{0}') {
        let record = record.trim_matches('\n');
        if record.is_empty() {
            continue;
        }
        let mut lines = record.lines();
        let header = match lines.next() {
            Some(h) => h,
            None => continue,
        };
        let mut fields = header.split('\u{1f}');
        let hash = fields.next().unwrap_or("").to_string();
        let author = fields.next().unwrap_or("").to_string();
        let subject = fields.next().unwrap_or("").to_string();

        let writes: Vec<String> = lines
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect();

        if hash.is_empty() {
            continue;
        }
        sets.push(ChangeSet {
            id: hash,
            agent: if author.is_empty() {
                None
            } else {
                Some(author)
            },
            message: if subject.is_empty() {
                None
            } else {
                Some(subject)
            },
            writes,
        });
    }

    // Oldest first, matching the causari ordering.
    sets.reverse();
    Ok(sets)
}

/// Apply built-in and user-defined rules to a set of changes.
fn apply_rules(changes: &[ChangeSet], cfg: &GuardConfig) -> Vec<AlertItem> {
    const CRITICAL_PATTERNS: [&str; 16] = [
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

    let mut items: Vec<AlertItem> = Vec::new();

    for ch in changes {
        let writes: HashSet<&str> = ch.writes.iter().map(|s| s.as_str()).collect();
        if writes.is_empty() {
            continue;
        }

        // Rule 1: bulk edit
        if writes.len() > 15 {
            items.push(AlertItem {
                id: ch.id.clone(),
                rule: "bulk edit".into(),
                detail: format!("touched {} files in a single change", writes.len()),
                agent: ch.agent.clone(),
                message: ch.message.clone(),
                severity: Severity::Alert,
            });
        }

        // Rule 2: critical file without test
        let has_critical = writes.iter().any(|w| {
            let low = w.to_lowercase();
            CRITICAL_PATTERNS.iter().any(|pat| low.contains(pat))
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
                    CRITICAL_PATTERNS.iter().any(|pat| low.contains(pat))
                })
                .cloned()
                .collect();
            items.push(AlertItem {
                id: ch.id.clone(),
                rule: "critical without test".into(),
                detail: format!("modified {} but no test file touched", crits.join(", ")),
                agent: ch.agent.clone(),
                message: ch.message.clone(),
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
                id: ch.id.clone(),
                rule: "missing tests".into(),
                detail: "modified source files but no tests".into(),
                agent: ch.agent.clone(),
                message: ch.message.clone(),
                severity: Severity::Warning,
            });
        }

        // User-defined rules from .causari/guard.toml
        for rule in &cfg.rules {
            let low_when = rule.when.to_lowercase();
            let matched = writes.iter().any(|w| w.to_lowercase().contains(&low_when));
            if matched {
                let threshold = rule.threshold.unwrap_or(1);
                if writes.len() >= threshold {
                    items.push(AlertItem {
                        id: ch.id.clone(),
                        rule: rule.name.clone(),
                        detail: format!(
                            "matched '{}' in {} files (threshold: {})",
                            rule.when,
                            writes.len(),
                            threshold
                        ),
                        agent: ch.agent.clone(),
                        message: ch.message.clone(),
                        severity: Severity::Alert,
                    });
                }
            }
        }
    }

    items
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

fn print_summary(items: &[AlertItem], alerts: usize, warnings: usize, source: &Source) {
    let status = if alerts > 0 {
        "❌ failing"
    } else if warnings > 0 {
        "⚠️ warnings"
    } else {
        "✅ passing"
    };
    let id_col = match source {
        Source::Causari => "Event",
        Source::Git => "Commit",
    };
    println!("## Causari Guard — {}", status);
    println!();
    println!("| {} | Author | Rule | Detail |", id_col);
    println!("|---|---|---|---|");
    for item in items {
        let short = short_id(&item.id);
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
    let scanned = match source {
        Source::Causari => "Causari event ledger",
        Source::Git => "git commit history",
    };
    println!(
        "<sub>Scanned {} · Powered by [Causari](https://causari.dev)</sub>",
        scanned
    );
}
