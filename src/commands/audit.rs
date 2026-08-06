/// `re audit` — retroactive Group-0 AI-code survival audit.
///
/// Works on any git repository without a Causari ledger. Reads git history,
/// classifies AI-authored commits by metadata, then counts how many of those
/// lines survived to HEAD.
use anyhow::{Context, Result, bail};
use colored::Colorize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::audit::{SurvivalReport, SurvivalStat, audit_repo};
use crate::cli::AuditArgs;

/// Best-effort temp-clone guard: removes the checkout when the audit is done.
struct TempClone(PathBuf);

impl Drop for TempClone {
    fn drop(&mut self) {
        // Git object files are read-only on Windows; clear attributes first.
        let _ = clear_readonly(&self.0);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn clear_readonly(dir: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let mut perms = entry.metadata()?.permissions();
        if perms.readonly() {
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            std::fs::set_permissions(&path, perms)?;
        }
        if path.is_dir() {
            clear_readonly(&path)?;
        }
    }
    Ok(())
}

/// Resolve the audit target: local path (default `.`), git URL, or GitHub
/// `owner/repo` shorthand. Remote targets are cloned into a temp directory
/// that is removed when the audit finishes.
fn resolve_target(target: Option<&str>) -> Result<(PathBuf, Option<TempClone>)> {
    let Some(raw) = target else {
        let cwd = std::env::current_dir().context("cannot determine current directory")?;
        return Ok((cwd, None));
    };

    let as_path = Path::new(raw);
    if as_path.exists() {
        return Ok((as_path.to_path_buf(), None));
    }

    let url =
        if raw.starts_with("http://") || raw.starts_with("https://") || raw.starts_with("git@") {
            raw.to_string()
        } else if raw.split('/').count() == 2 && !raw.contains(char::is_whitespace) {
            // GitHub shorthand: owner/repo
            format!("https://github.com/{raw}")
        } else {
            bail!("'{raw}' is neither an existing path, a git URL, nor an owner/repo shorthand");
        };

    let dest = std::env::temp_dir().join(format!("causari-audit-{}", std::process::id()));
    eprintln!("cloning {url} ...");
    let status = Command::new("git")
        .args(["clone", "--quiet", "--single-branch", &url])
        .arg(&dest)
        .status()
        .context("failed to run git clone")?;
    if !status.success() {
        bail!("git clone failed for {url}");
    }
    Ok((dest.clone(), Some(TempClone(dest))))
}

pub fn run(args: AuditArgs) -> Result<()> {
    let (dir, _tmp) = resolve_target(args.target.as_deref())?;
    let report = audit_repo(&dir).context("audit failed")?;

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

    if args.badge {
        let svg = generate_badge(&report);
        let path = Path::new("causari-badge.svg");
        std::fs::write(path, svg).with_context(|| format!("writing {}", path.display()))?;
        println!(
            "{} badge written to {} — embed it in your README:",
            "✓".green().bold(),
            path.display()
        );
        println!("    ![AI survival](./causari-badge.svg)");
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

/// Shields-style flat badge: `AI survival | NN.N%`.
fn generate_badge(report: &SurvivalReport) -> String {
    let v = &report.verified;
    let (value, color) = match v.survival_rate() {
        None => ("n/a".to_string(), "#9f9f9f"),
        Some(r) if r >= 0.70 => (format!("{:.1}%", r * 100.0), "#4c1"),
        Some(r) if r >= 0.40 => (format!("{:.1}%", r * 100.0), "#dfb317"),
        Some(r) => (format!("{:.1}%", r * 100.0), "#e05d44"),
    };
    let label = "AI survival";
    let label_w: u32 = 76;
    let value_w: u32 = 12 + value.len() as u32 * 8;
    let total_w = label_w + value_w;
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{total_w}" height="20" role="img" aria-label="{label}: {value}">
  <linearGradient id="s" x2="0" y2="100%"><stop offset="0" stop-color="#bbb" stop-opacity=".1"/><stop offset="1" stop-opacity=".1"/></linearGradient>
  <clipPath id="r"><rect width="{total_w}" height="20" rx="3" fill="#fff"/></clipPath>
  <g clip-path="url(#r)">
    <rect width="{label_w}" height="20" fill="#555"/>
    <rect x="{label_w}" width="{value_w}" height="20" fill="{color}"/>
    <rect width="{total_w}" height="20" fill="url(#s)"/>
  </g>
  <g fill="#fff" text-anchor="middle" font-family="Verdana,Geneva,DejaVu Sans,sans-serif" font-size="11">
    <text x="{lx}" y="14">{label}</text>
    <text x="{vx}" y="14">{value}</text>
  </g>
</svg>"##,
        lx = label_w / 2 + 1,
        vx = label_w + value_w / 2,
    )
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
