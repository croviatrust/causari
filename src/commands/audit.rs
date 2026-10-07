/// `re audit` — retroactive survival count for commits whose git metadata
/// matched an AI-detection rule. Matching metadata is not proof a model
/// wrote the line. The JSON field stays `verified`.
///
/// Works on any git repository without a Causari ledger. Counts how many
/// of those lines `git blame` still attributes to the commit at HEAD.
use anyhow::{Context, Result, bail};
use colored::Colorize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::audit::{
    AgeBucket, AuditOptions, Baseline, CAP_CEILING_LINES, IGNORE_REVS_FILE, METHOD_VERSION,
    ShallowCloneRefused, SurvivalReport, SurvivalStat, audit_repo,
};
use crate::audit_seal::{self, AuditBinding};
use crate::cli::AuditArgs;
use crate::exit::exit_with;
use crate::repo::Repo;
use crate::seal::SealIssuer;

const DEFAULT_SEAL_FILE: &str = "audit.seal.json";

/// Human label for the audit class whose JSON field is still `verified`.
const LABEL_TAGGED: &str = "AI-tagged (metadata matched)";
const LABEL_BY_AGENT: &str = "By agent (metadata matched only)";
const CARD_EMPTY: &str = "no AI-tagged commits";
const SUMMARY_FIELD_NOTE: &str =
    "The JSON field is still `verified`: metadata matched, not authorship proved.";

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
    if dest.exists() {
        // A crashed previous run can leave a stale checkout behind.
        let _ = clear_readonly(&dest);
        std::fs::remove_dir_all(&dest)
            .with_context(|| format!("cannot clear stale clone dir {}", dest.display()))?;
    }
    eprintln!("cloning {url} ...");
    let status = Command::new("git")
        .args(["clone", "--quiet", "--single-branch", &url])
        .arg(&dest)
        .status()
        .context("failed to run git clone")?;
    if !status.success() {
        bail!("git clone failed for {url}");
    }
    // `git clone` does not fetch notes; git-ai authorship logs live under
    // refs/notes/ai. Best effort: most repositories simply do not have it.
    let _ = Command::new("git")
        .args(["fetch", "--quiet", "origin", "+refs/notes/ai:refs/notes/ai"])
        .current_dir(&dest)
        .stderr(std::process::Stdio::null())
        .status();
    Ok((dest.clone(), Some(TempClone(dest))))
}

/// The machine-readable report: every class and agent carries the sums, the
/// line-weighted rate and the robust figures; `coverage` says how it was
/// measured and `repository` names what was measured — the commit at HEAD
/// and the origin label (credentials stripped, or a digest of the path when
/// there is no remote). The same `repository.head` is the same commit, not
/// a promise that a later audit matches: classification also reads
/// `refs/notes/ai`, which is not part of the commit, plus the method and
/// the blame flags in `coverage`. A repository that has moved has a
/// different head.
fn report_json(dir: &Path, report: &SurvivalReport) -> Result<serde_json::Value> {
    let mut value = serde_json::to_value(report)?;
    value["method"] = serde_json::json!(METHOD_VERSION);
    value["repository"] = serde_json::json!({
        "head": crate::audit::head_commit(dir)?,
        "origin": audit_seal::repo_label(dir),
    });
    Ok(value)
}

/// The exact bytes `--json` prints: pretty JSON and one newline. An audit
/// seal commits to these bytes, so they are produced in one place.
fn audit_json_bytes(dir: &Path, report: &SurvivalReport) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(&report_json(dir, report)?)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// The repository whose seal issuer signs an audit. A local audit signs
/// with the audited repository's own identity, so audit seals and exchange
/// seals of one project form one chain; `.causari/` is created there on
/// first use, as `re init` would. A temp clone has no identity of its own:
/// the current directory's repository signs.
fn issuer_repo(dir: &Path, is_temp_clone: bool) -> Result<Repo> {
    if !is_temp_clone {
        if let Ok(repo) = Repo::discover_from(dir) {
            return Ok(repo);
        }
    }
    if let Ok(repo) = Repo::discover() {
        return Ok(repo);
    }
    if is_temp_clone {
        bail!(
            "--seal needs an issuer identity: run `re init` in the directory that should sign \
             (its .causari/keys/seal-issuer.key and seal chain are used), then audit again"
        );
    }
    let repo = Repo::init(dir)?;
    let _ = repo.ensure_gitignored();
    eprintln!(
        "created {} for the seal issuer key and chain (gitignored)",
        repo.dir.display()
    );
    Ok(repo)
}

/// Issue the seal over `audit_json` and write the bundle to `out`.
fn seal_audit(
    dir: &Path,
    is_temp_clone: bool,
    args: &AuditArgs,
    report: &SurvivalReport,
    audit_json: &[u8],
) -> Result<(PathBuf, serde_json::Value)> {
    let binding = AuditBinding {
        commit: crate::audit::head_commit(dir)?,
        method: report.coverage.method.to_string(),
        allow_shallow: args.allow_shallow,
        shallow: report.coverage.shallow,
        repo: audit_seal::repo_label(dir),
    };
    let repo = issuer_repo(dir, is_temp_clone)?;
    let mut issuer = SealIssuer::load_or_create(&repo, None)?;
    let bundle = audit_seal::issue(&mut issuer, audit_json, &binding)?;
    let out = args
        .output
        .clone()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SEAL_FILE));
    let mut text = serde_json::to_string_pretty(&bundle)?;
    text.push('\n');
    std::fs::write(&out, text).with_context(|| format!("writing {}", out.display()))?;
    Ok((out, bundle))
}

pub fn run(args: AuditArgs) -> Result<()> {
    let (dir, tmp_clone) = resolve_target(args.target.as_deref())?;
    let opts = AuditOptions {
        allow_shallow: args.allow_shallow,
    };
    let report = audit_repo(&dir, &opts).map_err(|e| {
        if e.is::<ShallowCloneRefused>() {
            exit_with(2, e)
        } else {
            e.context("audit failed")
        }
    })?;

    let audit_json = if args.json || args.seal {
        Some(audit_json_bytes(&dir, &report)?)
    } else {
        None
    };
    // Seal before printing: a failed issuance must not leave a report on
    // stdout that looks sealed.
    let sealed = if args.seal {
        let bytes = audit_json.as_deref().unwrap_or_default();
        Some(seal_audit(
            &dir,
            tmp_clone.is_some(),
            &args,
            &report,
            bytes,
        )?)
    } else {
        None
    };

    if args.json {
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(audit_json.as_deref().unwrap_or_default())?;
        stdout.flush()?;
        if let Some((out, bundle)) = &sealed {
            // stdout carries exactly the sealed bytes; the notice goes elsewhere.
            eprintln!(
                "seal {} (sequence {}) written to {}",
                bundle["seal"]["seal_id"].as_str().unwrap_or("?"),
                bundle["seal"]["chain"]["sequence"],
                out.display()
            );
        }
        return Ok(());
    }

    let head = crate::audit::head_commit(&dir).ok();
    if args.summary {
        print_summary(&report, head.as_deref());
    } else {
        print_terminal(&report, head.as_deref());
        // The ledger is a different store. Say so once, only for a working
        // tree that does not already have one, so `re report` is not the
        // next step after this audit.
        if tmp_clone.is_none() && !dir.join(".causari").is_dir() {
            println!();
            println!(
                "  {} this audit did not use a ledger. `re report` reads a local ledger and fails in this directory.",
                "ledger:".bright_black()
            );
            println!("         `re init` creates that ledger. It does not export the audit above.");
        }
    }

    if let Some((out, bundle)) = &sealed {
        let seal = &bundle["seal"];
        println!();
        if args.summary {
            // The summary is Markdown for a PR comment or job summary; the
            // seal note is one paragraph of it.
            println!(
                "Sealed: `{}` (crovia.seal.v1) over this audit of `{}`, written to `{}`. \
                 Verify offline with `re seal verify {}` or at https://causari.dev/verify — \
                 the seal proves these exact numbers were signed for this commit, not that they are true.",
                seal["seal_id"].as_str().unwrap_or("?"),
                seal["generator"]["params"]["commit"]
                    .as_str()
                    .map(|c| &c[..c.len().min(12)])
                    .unwrap_or("?"),
                out.display(),
                out.display()
            );
        } else {
            println!(
                "{} seal {} written to {}",
                "✓".green().bold(),
                seal["seal_id"].as_str().unwrap_or("?").cyan(),
                out.display()
            );
            println!(
                "  issuer   {}  (sequence {})",
                seal["issuer"]["id"].as_str().unwrap_or("?"),
                seal["chain"]["sequence"]
            );
            println!(
                "  commit   {}  method {}",
                seal["generator"]["params"]["commit"]
                    .as_str()
                    .unwrap_or("?"),
                seal["generator"]["params"]["method"]
                    .as_str()
                    .unwrap_or("?")
            );
            println!(
                "  verify   {} — or drop the file on https://causari.dev/verify",
                format!("re seal verify {}", out.display()).cyan()
            );
        }
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
        let mut snapshot = report_json(&dir, &report)?;
        snapshot["timestamp"] = serde_json::json!(chrono::Utc::now().to_rfc3339());
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

/// Under the first percentage: what the rate is, and what a missing tag is not.
fn print_survival_reading(report: &SurvivalReport) {
    if report.verified.commits == 0 {
        println!(
            "  No metadata-matched commits. That is not a finding that the repository \
             contains no AI-written lines. UNKNOWN and untagged are commits with no such \
             signal; they are not a finding that a human wrote them."
        );
        return;
    }
    println!("  {}", survival_reading(report));
}

fn survival_reading(report: &SurvivalReport) -> String {
    format!(
        "Of the lines introduced by metadata-matched commits, this percentage is the share \
         `git blame {}` still attributes to those commits. It is not the share of the \
         repository and it is not a quality score. Metadata matched does not prove who \
         wrote each line. UNKNOWN and untagged are commits with no such signal; they are \
         not a finding that a human wrote them.",
        report.coverage.blame_flags.join(" ")
    )
}

/// `gap` is tagged line-weighted survival minus untagged line-weighted survival,
/// in rate units. Printed in percentage points, with the direction named.
fn age_matched_sentence(b: &Baseline, floor: u64) -> String {
    match &b.age_matched {
        Some(m) => format!(
            "Age-matched gap: {:+.1} percentage points (AI-tagged {} minus untagged {} in the matched age windows). \
             Positive means the metadata-matched lines have the higher line-weighted survival in those windows, \
             over {} matched age window{} holding {:.0}% of the AI-tagged lines. \
             Lines outside those windows are not in this gap.",
            m.gap * 100.0,
            pct(Some(m.tagged_rate)),
            pct(Some(m.untagged_rate)),
            m.buckets_used,
            plural(m.buckets_used as u64),
            m.tagged_lines_covered * 100.0,
        ),
        None => format!(
            "Age-matched gap: unavailable. No age window holds at least {floor} commits of both \
             kinds, so there is no comparison across matched age windows."
        ),
    }
}

fn below_floor_sentence(commits: u64, floor: u64) -> String {
    format!(
        "below floor: {commits} commit{} (floor {floor}); not comparable. \
         Meeting the floor is not a reliability guarantee.",
        plural(commits),
    )
}

fn print_export_footer(report: &SurvivalReport, head: Option<&str>, markdown: bool) {
    let flags = report.coverage.blame_flags.join(" ");
    let head_text = match head {
        Some(h) => format!("commit `{h}`"),
        None => "the commit in `repository.head`".to_string(),
    };
    if markdown {
        println!();
        println!(
            "Read the age-matched gap as the comparison. An agent line marked below floor is not \
             comparable. Meeting the floor is not a reliability guarantee."
        );
        println!();
        println!(
            "Export: `re audit <target> --json` (the counts and {head_text}), `--summary` for this \
             Markdown, `--seal` to sign those JSON bytes for that commit and method {}. \
             `--badge` and `--card` write SVGs. `re report` is the local ledger, not this audit.",
            report.coverage.method
        );
        println!();
        println!(
            "The export stores the counts, {head_text}, method {}, blame `{flags}`, and the coverage flags. \
             It does not store `refs/notes/ai`. That ref is not part of the commit; adding or removing \
             it can change the counts while the stored SHA stays the same. A repository that has moved \
             has a different commit, and this export does not reproduce it.",
            report.coverage.method
        );
        return;
    }
    println!();
    println!("{}", "Reading and export".bright_black().bold());
    println!(
        "  · The age-matched gap above is the comparison. An agent row marked below floor is not comparable."
    );
    println!("    Meeting the floor is not a reliability guarantee.");
    println!(
        "  · Export: `re audit <target> --json` (counts and {head_text}), `--summary` for Markdown,"
    );
    println!(
        "    `--seal` signs those JSON bytes for that commit and method {}. `--badge` and `--card` write SVGs.",
        report.coverage.method
    );
    println!("    `re report` reads the local ledger. It does not export this audit.");
    println!(
        "  · Stored in `--json` and in `--seal`: the counts, {head_text}, method {}, blame `{flags}`,",
        report.coverage.method
    );
    println!(
        "    plus coverage flags (shallow, ignore-revs, sample floor). `refs/notes/ai` is not stored."
    );
    println!(
        "    That ref is not part of the commit. Adding or removing it can change the counts while the SHA stays."
    );
    println!(
        "    A repository that has moved has a different commit, and this export does not reproduce it."
    );
}

fn print_terminal(report: &SurvivalReport, head: Option<&str>) {
    println!("{}", "∵ causari · AI code survival".bold());
    println!(
        "{}",
        "───────────────────────────────────────────────────".bright_black()
    );
    println!(
        "  {} commits analyzed (git metadata only, no setup required)",
        report.total_commits
    );
    println!();

    print_class(LABEL_TAGGED, &report.verified, false);
    print_survival_reading(report);
    println!(
        "  {}",
        age_matched_sentence(&report.baseline, report.coverage.sample_floor)
    );
    if let Some(sentence) = dominance_sentence(&report.verified) {
        println!("  {sentence}");
    }
    print_class("Probable AI-assisted", &report.probable, true);

    if !report.by_agent.is_empty() {
        println!("{}", LABEL_BY_AGENT.bold());
        println!(
            "  {:20} {:>7} {:>10} {:>9} {:>8} {:>8} {:>8}",
            "agent", "commits", "introduced", "survived", "line-wt", "capped", "median"
        );
        for (agent, stat) in &report.by_agent {
            println!(
                "  {:20} {:>7} {:>10} {:>9} {:>8} {:>8} {:>8}",
                agent.cyan(),
                stat.commits,
                stat.introduced,
                stat.surviving,
                pct(stat.survival_rate()),
                pct(stat.capped_survival_rate()),
                pct(stat.median_survival()),
            );
            if let Some(sentence) = dominance_sentence(stat) {
                println!("    {sentence}");
            }
            if stat.commits < report.coverage.sample_floor {
                println!(
                    "    {}",
                    below_floor_sentence(stat.commits, report.coverage.sample_floor)
                );
            }
        }
    }

    print_baseline(&report.baseline, report.coverage.sample_floor);

    println!();
    println!("{}", "Confidence notes".bright_black().bold());
    println!(
        "  · JSON field `verified` = metadata matched (trailers, bot author, …), not authorship proved"
    );
    println!("  · PROBABLE = weak heuristic; may include human-assisted commits");
    println!("  · UNKNOWN commits are excluded from headline numbers; they form");
    println!("    the untagged baseline (human, inline-completed and untagged-agent code alike)");
    println!("  · line-wt = Σ surviving / Σ introduced; capped = same, with each commit");
    println!(
        "    weighing at most min(p95 of per-commit introduced lines, {} lines);",
        CAP_CEILING_LINES
    );
    println!("    median = median of per-commit rates");
    if report.coverage.small_sample {
        println!(
            "  · Small sample: {} verified commit{} (floor {}). Read the figures as counts.",
            report.verified.commits,
            plural(report.verified.commits),
            report.coverage.sample_floor
        );
    }
    println!("  · Only lines from AI-tagged commits are measured; inline completions");
    println!("    (Copilot, Cursor Tab, …) leave no git trace and are invisible here");
    println!(
        "  · blame {}{}",
        report.coverage.blame_flags.join(" "),
        if report.coverage.ignore_revs_file {
            format!(" --ignore-revs-file={IGNORE_REVS_FILE}")
        } else {
            String::new()
        }
    );
    if report.coverage.shallow {
        println!("  · Shallow clone: history is truncated, the figures above are partial");
    }
    println!(
        "  · A measurement, not a grade: method {} at https://causari.dev/method",
        report.coverage.method
    );
    print_export_footer(report, head, false);
}

fn print_summary(report: &SurvivalReport, head: Option<&str>) {
    let v = &report.verified;

    // A measurement, not a grade: no colour, no verdict. The reader judges.
    println!("## ∵ causari · AI code survival");
    println!();
    println!(
        "{} commits analyzed (git metadata only, retroactive, no setup).",
        report.total_commits
    );
    println!();
    if report.coverage.shallow {
        println!(
            "_Shallow clone: history is truncated and these figures are partial. \
             Use `fetch-depth: 0` or `git fetch --unshallow` for a full measurement._"
        );
        println!();
    }

    if v.commits > 0 {
        println!("{}", tagged_summary_bold(v));
        println!();
        println!("{}", survival_reading(report));
        println!();
        println!(
            "{}",
            age_matched_sentence(&report.baseline, report.coverage.sample_floor)
        );
        if let Some(sentence) = dominance_sentence(v) {
            println!();
            println!("_{sentence}._");
        }
        if report.coverage.small_sample {
            println!();
            println!(
                "_Small sample: {} AI-tagged commit{} (floor {}). Read the figures as counts, not rates. Meeting the floor is not a reliability guarantee._",
                v.commits,
                plural(v.commits),
                report.coverage.sample_floor
            );
        }
        println!();
        summary_baseline(&report.baseline);
    } else {
        println!(
            "No metadata-matched commits. That is not a finding that the repository contains no AI-written lines."
        );
        println!();
        println!(
            "{}",
            age_matched_sentence(&report.baseline, report.coverage.sample_floor)
        );
        println!();
    }
    if report.probable.commits > 0 {
        println!(
            "Probable AI-assisted: {} commits, {} introduced, {} survived ({} line-weighted · {} capped · median {}).",
            report.probable.commits,
            report.probable.introduced,
            report.probable.surviving,
            pct(report.probable.survival_rate()),
            pct(report.probable.capped_survival_rate()),
            pct(report.probable.median_survival()),
        );
        println!();
    }

    if !report.by_agent.is_empty() {
        println!("| Agent | Commits | Introduced | Survived | Line-weighted | Capped | Median |");
        println!("|---|---:|---:|---:|---:|---:|---:|");
        for (agent, stat) in &report.by_agent {
            println!(
                "| {} | {} | {} | {} | {} | {} | {} |",
                agent,
                stat.commits,
                stat.introduced,
                stat.surviving,
                pct(stat.survival_rate()),
                pct(stat.capped_survival_rate()),
                pct(stat.median_survival()),
            );
        }
        let dominated: Vec<String> = report
            .by_agent
            .iter()
            .filter_map(|(agent, stat)| dominance_sentence(stat).map(|s| format!("{agent}: {s}")))
            .collect();
        let below: Vec<String> = report
            .by_agent
            .iter()
            .filter(|(_, stat)| stat.commits < report.coverage.sample_floor)
            .map(|(agent, stat)| {
                format!(
                    "{agent}: {}",
                    below_floor_sentence(stat.commits, report.coverage.sample_floor)
                )
            })
            .collect();
        if !dominated.is_empty() || !below.is_empty() {
            println!();
            for line in dominated {
                println!("_{line}._  ");
            }
            for line in below {
                let line = line.trim_end_matches('.');
                println!("_{line}._  ");
            }
        }
        println!();
    }

    println!(
        "<sub>PROBABLE = heuristic. \
         Counts lines from AI-tagged commits still attributed to them by `git blame {}`; \
         inline completions leave no git trace and are not measured. \
         Capped: each commit weighs at most min(p95 of per-commit introduced lines, {} lines); \
         median: median of per-commit rates. \
         Untagged = commits with no AI signal (human, inline-completed and untagged-agent code alike); \
         age = commit date to HEAD date. \
         Method {}: [causari.dev/method](https://causari.dev/method). \
         `--json` stores the commit, method, blame flags and counts. It does not store `refs/notes/ai`. \
         {}</sub>",
        report.coverage.blame_flags.join(" "),
        CAP_CEILING_LINES,
        report.coverage.method,
        SUMMARY_FIELD_NOTE,
    );
    print_export_footer(report, head, true);
}

/// The bold line of `re audit --summary`. Metadata matched, not authorship.
fn tagged_summary_bold(v: &SurvivalStat) -> String {
    format!(
        "**AI-tagged (metadata matched): {}** line-weighted ({} of {} lines still at HEAD, {} commit{}) · {} capped · median {}",
        pct(v.survival_rate()),
        v.surviving,
        v.introduced,
        v.commits,
        plural(v.commits),
        pct(v.capped_survival_rate()),
        pct(v.median_survival()),
    )
}

/// The Markdown counterpart of [`print_baseline`]: one paragraph, one
/// table when there is something to put side by side.
fn summary_baseline(b: &Baseline) {
    if b.untagged.commits == 0 {
        println!(
            "Every commit carries an AI tag; there is no untagged code in this repository to compare with."
        );
        println!();
        return;
    }
    println!(
        "Same repository, untagged lines: {} of {} still at HEAD ({} line-weighted, {} commit{}).",
        b.untagged.surviving,
        b.untagged.introduced,
        pct(b.untagged.survival_rate()),
        b.untagged.commits,
        plural(b.untagged.commits),
    );
    let rows: Vec<&AgeBucket> = b
        .by_age
        .iter()
        .filter(|r| r.tagged.commits + r.untagged.commits > 0)
        .collect();
    if rows.len() > 1 {
        println!();
        println!("| Line age | AI-tagged | Untagged |");
        println!("|---|---:|---:|");
        for r in rows {
            println!(
                "| {} | {} | {} |",
                r.label(),
                cohort_cell(&r.tagged),
                cohort_cell(&r.untagged)
            );
        }
    }
    if let Some(o) = &b.oldest_surviving
        && o.commits_before > 0
    {
        println!();
        println!(
            "_The oldest line still at HEAD dates {}; {} commit{} ({} AI-tagged) are older and nothing from before that date survives, tagged or not._",
            o.date,
            o.commits_before,
            plural(o.commits_before),
            o.tagged_commits_before,
        );
    }
    println!();
}

/// The same repository's untagged lines, by age, next to the AI-tagged ones.
fn print_baseline(b: &Baseline, floor: u64) {
    println!();
    println!(
        "{}",
        "Baseline: untagged lines of the same repository".bold()
    );
    if b.untagged.commits == 0 {
        println!("  every commit carries an AI tag; there is no untagged code to compare with");
    } else {
        println!(
            "  untagged: {} commits, {} introduced, {} survived · {} line-weighted · {} median",
            b.untagged.commits,
            b.untagged.introduced,
            b.untagged.surviving,
            pct(b.untagged.survival_rate()),
            pct(b.untagged.median_survival()),
        );
    }
    let rows: Vec<&AgeBucket> = b
        .by_age
        .iter()
        .filter(|r| r.tagged.commits + r.untagged.commits > 0)
        .collect();
    if !rows.is_empty() {
        println!(
            "  {:>10} {:>24} {:>24}",
            "line age", "AI-tagged", "untagged"
        );
        for r in rows {
            println!(
                "  {:>10} {:>24} {:>24}{}",
                r.label(),
                cohort_cell(&r.tagged),
                cohort_cell(&r.untagged),
                if r.comparable() {
                    ""
                } else {
                    "   (below floor on one side)"
                }
            );
        }
    }
    println!("  {}", age_matched_sentence(b, floor));
    if let Some(o) = &b.oldest_surviving
        && o.commits_before > 0
    {
        println!(
            "  oldest line still at HEAD dates {} ({} days); {} commit{} ({} lines, {} AI-tagged commit{} \
             with {} lines) are older: nothing from before that date survives, tagged or not",
            o.date,
            o.age_days,
            o.commits_before,
            plural(o.commits_before),
            o.introduced_before,
            o.tagged_commits_before,
            plural(o.tagged_commits_before),
            o.tagged_introduced_before,
        );
    }
}

fn cohort_cell(stat: &SurvivalStat) -> String {
    if stat.commits == 0 {
        return "—".into();
    }
    format!(
        "{} ({} commit{})",
        pct(stat.survival_rate()),
        stat.commits,
        plural(stat.commits)
    )
}

fn print_class(label: &str, stat: &SurvivalStat, dominance: bool) {
    if stat.commits == 0 {
        println!("{}: {}", label.bold(), "none detected".bright_black());
        return;
    }
    println!(
        "{}: {} commits, {} introduced, {} survived",
        label.bold(),
        stat.commits,
        stat.introduced,
        stat.surviving,
    );
    println!(
        "  survival {} line-weighted · {} capped · median {}",
        pct(stat.survival_rate()),
        pct(stat.capped_survival_rate()),
        pct(stat.median_survival()),
    );
    if dominance {
        if let Some(sentence) = dominance_sentence(stat) {
            println!("  {sentence}");
        }
    }
}

/// One plain sentence when a single commit holds at least half of a row's
/// introduced lines: the row then measures that commit, and the reader
/// should know before comparing it with anything.
fn dominance_sentence(stat: &SurvivalStat) -> Option<String> {
    if !stat.dominated_by_one_commit() {
        return None;
    }
    let share = stat.largest_commit_share()?;
    Some(format!(
        "one commit accounts for {:.0}% of introduced lines; this row measures that commit",
        share * 100.0
    ))
}

fn pct(rate: Option<f64>) -> String {
    match rate {
        Some(r) => format!("{:.1}%", r * 100.0),
        None => "n/a".into(),
    }
}

fn plural(n: u64) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// The identity palette. Numbers never carry colour: a badge or a card reports
/// a measurement, it does not grade it, so every value renders in graphite.
const INK: &str = "#0b0d10";
const PAPER: &str = "#f5f4ef";
const GRAPHITE: &str = "#3b4252";
const MIST: &str = "#9aa3ad";
const MONO: &str = "ui-monospace,'JetBrains Mono','SF Mono','Cascadia Mono',Menlo,Consolas,'DejaVu Sans Mono',monospace";

/// The ∵ mark as three discs, so it renders identically in every viewer and
/// never depends on a font carrying U+2235.
fn mark_svg(x: f32, y: f32, size: f32, fill: &str) -> String {
    let s = size / 100.0;
    let r = 14.5 * s;
    [(28.0, 34.0), (72.0, 34.0), (50.0, 72.0)]
        .iter()
        .map(|(cx, cy): &(f32, f32)| {
            format!(
                r#"<circle cx="{:.2}" cy="{:.2}" r="{:.2}" fill="{fill}"/>"#,
                x + cx * s,
                y + cy * s,
                r
            )
        })
        .collect::<Vec<_>>()
        .join("")
}

/// Shields-style flat badge: `∵ AI survival | NN.N%`.
fn generate_badge(report: &SurvivalReport) -> String {
    let v = &report.verified;
    let value = match v.survival_rate() {
        None => "n/a".to_string(),
        Some(r) => format!("{:.1}%", r * 100.0),
    };
    let label = "AI survival";
    let label_w: u32 = 102;
    let value_w: u32 = 16 + value.len() as u32 * 7;
    let total_w = label_w + value_w;
    let mark = mark_svg(5.0, 4.0, 12.0, PAPER);
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{total_w}" height="20" role="img" aria-label="∵ {label}: {value}">
  <clipPath id="r"><rect width="{total_w}" height="20" rx="3" fill="#fff"/></clipPath>
  <g clip-path="url(#r)">
    <rect width="{label_w}" height="20" fill="{INK}"/>
    <rect x="{label_w}" width="{value_w}" height="20" fill="{GRAPHITE}"/>
  </g>
  {mark}
  <g fill="{PAPER}" font-family="{MONO}" font-size="11">
    <text x="22" y="14">{label}</text>
    <text x="{vx}" y="14" text-anchor="middle">{value}</text>
  </g>
</svg>"##,
        vx = label_w + value_w / 2,
    )
}

fn generate_svg_card(report: &SurvivalReport) -> String {
    let v = &report.verified;
    let (headline, detail) = match v.survival_rate() {
        None => (
            CARD_EMPTY.to_string(),
            "nothing to measure from git metadata".to_string(),
        ),
        Some(r) => (
            format!("{:.1}% still at HEAD", r * 100.0),
            format!(
                "{} of {} lines, {} commits",
                v.surviving, v.introduced, v.commits
            ),
        ),
    };
    let sample_note = if v.commits > 0 && v.commits < 5 {
        " · small sample"
    } else {
        ""
    };
    let mark = mark_svg(36.0, 30.0, 28.0, PAPER);
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="560" height="240" viewBox="0 0 560 240" role="img" aria-label="∵ causari · AI code survival: {headline}">
  <rect width="560" height="240" rx="12" fill="{INK}"/>
  {mark}
  <text x="74" y="52" fill="{PAPER}" font-family="{MONO}" font-size="16" font-weight="500">causari <tspan fill="{MIST}">· AI code survival</tspan></text>
  <text x="36" y="118" fill="{PAPER}" font-family="{MONO}" font-size="30" font-weight="500">{headline}</text>
  <text x="36" y="148" fill="{MIST}" font-family="{MONO}" font-size="13">{detail}{sample_note}</text>
  <text x="36" y="172" fill="{MIST}" font-family="{MONO}" font-size="13">probable AI-assisted: {probable} commits, excluded from the number above</text>
  <line x1="36" y1="192" x2="524" y2="192" stroke="{GRAPHITE}" stroke-width="1"/>
  <text x="36" y="214" fill="{MIST}" font-family="{MONO}" font-size="11">git metadata only · a count, not a grade · re audit · method {method} · causari.dev/method</text>
</svg>"##,
        probable = report.probable.commits,
        method = report.coverage.method,
    )
}

#[cfg(test)]
mod wording_tests {
    use super::*;

    #[test]
    fn labels_say_metadata_matched_and_keep_the_json_field_name() {
        assert_eq!(LABEL_TAGGED, "AI-tagged (metadata matched)");
        assert_eq!(LABEL_BY_AGENT, "By agent (metadata matched only)");
        assert_eq!(CARD_EMPTY, "no AI-tagged commits");
        assert!(SUMMARY_FIELD_NOTE.contains("`verified`"));
        assert!(SUMMARY_FIELD_NOTE.contains("metadata matched"));
        assert!(SUMMARY_FIELD_NOTE.contains("not authorship proved"));
        let mut stat = SurvivalStat::default();
        stat.record(10, 4);
        let line = tagged_summary_bold(&stat);
        assert!(line.starts_with("**AI-tagged (metadata matched):"));
        assert!(!line.contains("Verified"));
        assert!(!line.to_lowercase().contains("proven"));
    }
}
