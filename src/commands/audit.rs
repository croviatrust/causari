/// `re audit` — retroactive Group-0 AI-code survival audit.
///
/// Works on any git repository without a Causari ledger. Reads git history,
/// classifies AI-authored commits by metadata, then counts how many of those
/// lines survived to HEAD.
use anyhow::{Context, Result};
use colored::Colorize;
use std::io::Write;
use std::path::Path;

use crate::audit::{SurvivalReport, SurvivalStat, audit_repo};
use crate::cli::AuditArgs;

pub fn run(args: AuditArgs) -> Result<()> {
    let cwd = std::env::current_dir().context("cannot determine current directory")?;
    let report = audit_repo(&cwd).context("audit failed")?;

    if args.json {
        serde_json::to_writer_pretty(
            std::io::stdout(),
            &serde_json::json!({
                "total_commits": report.total_commits,
                "verified": {
                    "commits": report.verified.commits,
                    "introduced": report.verified.introduced,
                    "surviving": report.verified.surviving,
                    "survival_rate": report.verified.survival_rate(),
                },
                "probable": {
                    "commits": report.probable.commits,
                    "introduced": report.probable.introduced,
                    "surviving": report.probable.surviving,
                    "survival_rate": report.probable.survival_rate(),
                },
                "by_agent": report.by_agent,
            }),
        )?;
        println!();
        return Ok(());
    }

    if args.summary {
        print_summary(&report);
    } else {
        print_terminal(&report);
    }

    if args.card {
        let svg = generate_svg_card(&report);
        let path = Path::new("causari-survival.svg");
        std::fs::write(path, svg).with_context(|| format!("writing {}", path.display()))?;
        println!(
            "{} survival card written to {}",
            "✓".green().bold(),
            path.display()
        );
    }

    if args.save {
        let snapshot = serde_json::json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "total_commits": report.total_commits,
            "verified": {
                "commits": report.verified.commits,
                "introduced": report.verified.introduced,
                "surviving": report.verified.surviving,
                "survival_rate": report.verified.survival_rate(),
            },
            "probable": {
                "commits": report.probable.commits,
                "introduced": report.probable.introduced,
                "surviving": report.probable.surviving,
                "survival_rate": report.probable.survival_rate(),
            },
            "by_agent": report.by_agent,
        });
        let path = Path::new(".causari/survival-snapshots.jsonl");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("opening {}", path.display()))?;
        writeln!(file, "{}", serde_json::to_string(&snapshot)?)
            .with_context(|| format!("writing {}", path.display()))?;
        println!(
            "{} snapshot saved to {}",
            "✓".green().bold(),
            path.display()
        );
    }

    Ok(())
}

fn print_terminal(report: &SurvivalReport) {
    println!("{}", "Causari Survival Audit".bold());
    println!(
        "{}",
        "═══════════════════════════════════════════════════".bright_black()
    );
    println!(
        "  {} commits analyzed (git-only, no Causari setup required)",
        report.total_commits
    );
    println!();

    print_class("Verified AI-authored", &report.verified);
    print_class("Probable AI-assisted", &report.probable);

    if !report.by_agent.is_empty() {
        println!("{}", "By agent (verified only)".bold());
        for (agent, stat) in &report.by_agent {
            println!(
                "  {:20} {:>6} lines, {:>6} survived ({:>5.1}%)",
                agent.cyan(),
                stat.introduced,
                stat.surviving,
                stat.survival_rate().unwrap_or(0.0) * 100.0
            );
        }
    }

    println!();
    println!("{}", "Confidence notes".bright_black().bold());
    println!("  · VERIFIED = explicit metadata (trailers, bot author, etc.)");
    println!("  · PROBABLE = weak heuristic; may include human-assisted commits");
    println!("  · UNKNOWN commits are excluded from headline numbers");
}

fn print_summary(report: &SurvivalReport) {
    let v = &report.verified;
    let rate = v.survival_rate();
    let status = if v.commits == 0 {
        "ℹ️ no verified AI commits"
    } else if rate.unwrap_or(0.0) >= 0.70 {
        "🟢 healthy"
    } else if rate.unwrap_or(0.0) >= 0.40 {
        "🟡 moderate churn"
    } else {
        "🔴 high churn"
    };

    println!("## Causari Survival Audit — {}", status);
    println!();
    println!(
        "{} commits analyzed (git-only, retroactive — no setup required).",
        report.total_commits
    );
    println!();

    if v.commits > 0 {
        println!(
            "**Verified AI survival: {:.1}%** ({} of {} lines still at HEAD, {} commits)",
            rate.unwrap_or(0.0) * 100.0,
            v.surviving,
            v.introduced,
            v.commits
        );
        println!();
    }
    if report.probable.commits > 0 {
        println!(
            "Probable AI-assisted: {} commits, {} introduced, {} survived ({:.1}%).",
            report.probable.commits,
            report.probable.introduced,
            report.probable.surviving,
            report.probable.survival_rate().unwrap_or(0.0) * 100.0
        );
        println!();
    }

    if !report.by_agent.is_empty() {
        println!("| Agent | Introduced | Survived | Survival |");
        println!("|---|---:|---:|---:|");
        for (agent, stat) in &report.by_agent {
            println!(
                "| {} | {} | {} | {:.1}% |",
                agent,
                stat.introduced,
                stat.surviving,
                stat.survival_rate().unwrap_or(0.0) * 100.0
            );
        }
        println!();
    }

    println!(
        "<sub>VERIFIED = explicit commit metadata; PROBABLE = heuristic. \
         Powered by [Causari](https://causari.dev) `re audit`</sub>"
    );
}

fn print_class(label: &str, stat: &SurvivalStat) {
    if stat.commits == 0 {
        println!("{}: {}", label.bold(), "none detected".bright_black());
        return;
    }
    let pct = stat.survival_rate().unwrap_or(0.0) * 100.0;
    println!(
        "{}: {} commits, {} introduced, {} survived ({:.1}%)",
        label.bold(),
        stat.commits,
        stat.introduced,
        stat.surviving,
        pct
    );
}

fn generate_svg_card(report: &SurvivalReport) -> String {
    let v = &report.verified;
    let pct = v.survival_rate().unwrap_or(0.0) * 100.0;
    let color = if pct >= 70.0 {
        "#22c55e"
    } else if pct >= 40.0 {
        "#eab308"
    } else {
        "#ef4444"
    };
    let verified = if v.commits == 0 {
        "No verified AI commits detected".to_string()
    } else {
        format!(
            "{:.1}% survival\n{} / {} lines",
            pct, v.surviving, v.introduced
        )
    };

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="440" height="240" viewBox="0 0 440 240">
  <rect width="440" height="240" rx="12" fill="#0f0f15"/>
  <rect x="20" y="20" width="400" height="200" rx="10" fill="none" stroke="{color}" stroke-width="2"/>
  <text x="40" y="60" fill="#a7f3d0" font-family="monospace" font-size="14" font-weight="bold">CAUSARI SURVIVAL AUDIT</text>
  <text x="40" y="110" fill="white" font-family="monospace" font-size="32" font-weight="bold">{verified}</text>
  <text x="40" y="150" fill="#94a3b8" font-family="monospace" font-size="12">Verified AI commits: {}</text>
  <text x="40" y="175" fill="#94a3b8" font-family="monospace" font-size="12">Probable AI commits: {}</text>
  <text x="40" y="205" fill="#64748b" font-family="monospace" font-size="10">Verified with Causari · causari.dev</text>
</svg>"##,
        v.commits, report.probable.commits
    )
}
