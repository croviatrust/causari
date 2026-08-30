//! Group-0 audit engine: retroactive AI-code survival from git history alone.
//!
//! Design rule ("blood type 0"): this module depends ONLY on `git` and the
//! filesystem. No agent hooks, no proxy, no Causari ledger. Integrations can
//! *improve* the data later; they must never be required for it to work.
//!
//! Pipeline:
//!   1. parse commit metadata            -> `CommitMeta`
//!   2. classify each commit             -> `Detection` (verified / probable)
//!   3. count lines introduced per commit (git numstat)
//!   4. count lines surviving at HEAD     (git blame porcelain)
//!   5. aggregate                        -> `SurvivalReport`
//!
//! Every number carries its evidence class. A commit with no machine-readable
//! authorship signal is UNKNOWN and never enters the headline figures.

use anyhow::{Context, Result};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::process::Command;

/// How sure we are that a commit is AI-authored, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct Detection {
    pub agent: String,
    /// 1.0 = explicit machine-readable metadata; below 0.9 = probable.
    pub confidence: f64,
    pub evidence: Vec<String>,
}

/// Evidence class thresholds.
pub const VERIFIED_THRESHOLD: f64 = 0.9;
pub const PROBABLE_THRESHOLD: f64 = 0.5;

/// Minimal commit metadata needed for detection.
#[derive(Debug, Clone)]
pub struct CommitMeta {
    pub hash: String,
    pub author_name: String,
    pub author_email: String,
    /// Full commit message including trailers.
    pub message: String,
}

/// Classify a commit as AI-authored (or not) from its metadata alone.
///
/// Detectors are ordered strongest-first; the first match wins. Signals:
/// - `Co-Authored-By: Claude`      -> claude-code, verified (1.0)
/// - `Co-Authored-By: ... Copilot` -> github-copilot, verified (1.0)
/// - aider author/committer marker -> aider, verified (0.95)
/// - known bot author emails       -> named bot, verified (0.95)
/// - `(aider)` suffix in message   -> aider, probable (0.7)
pub fn detect_ai(commit: &CommitMeta) -> Option<Detection> {
    let msg_lower = commit.message.to_lowercase();
    let author_lower = commit.author_name.to_lowercase();
    let email_lower = commit.author_email.to_lowercase();

    // Trailer-based detection: scan message lines for co-author trailers.
    for line in commit.message.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();
        if let Some(rest) = lower.strip_prefix("co-authored-by:") {
            let rest = rest.trim();
            if rest.starts_with("claude") || rest.contains("noreply@anthropic.com") {
                return Some(Detection {
                    agent: "claude-code".into(),
                    confidence: 1.0,
                    evidence: vec![format!("trailer: {}", trimmed)],
                });
            }
            if rest.contains("copilot") {
                return Some(Detection {
                    agent: "github-copilot".into(),
                    confidence: 1.0,
                    evidence: vec![format!("trailer: {}", trimmed)],
                });
            }
            if rest.contains("cursor") {
                return Some(Detection {
                    agent: "cursor".into(),
                    confidence: 1.0,
                    evidence: vec![format!("trailer: {}", trimmed)],
                });
            }
            if rest.contains("aider") {
                return Some(Detection {
                    agent: "aider".into(),
                    confidence: 1.0,
                    evidence: vec![format!("trailer: {}", trimmed)],
                });
            }
            if rest.contains("codex") || rest.contains("chatgpt") {
                return Some(Detection {
                    agent: "openai-codex".into(),
                    confidence: 1.0,
                    evidence: vec![format!("trailer: {}", trimmed)],
                });
            }
            if rest.contains("gemini") {
                return Some(Detection {
                    agent: "gemini".into(),
                    confidence: 1.0,
                    evidence: vec![format!("trailer: {}", trimmed)],
                });
            }
            if rest.contains("openhands") {
                return Some(Detection {
                    agent: "openhands".into(),
                    confidence: 1.0,
                    evidence: vec![format!("trailer: {}", trimmed)],
                });
            }
            if rest.contains("devin") {
                return Some(Detection {
                    agent: "devin".into(),
                    confidence: 1.0,
                    evidence: vec![format!("trailer: {}", trimmed)],
                });
            }
            if rest.starts_with("jules") || rest.contains("jules@google") {
                return Some(Detection {
                    agent: "jules".into(),
                    confidence: 1.0,
                    evidence: vec![format!("trailer: {}", trimmed)],
                });
            }
        }
    }

    // Author-identity detection.
    if author_lower.contains("(aider)") || email_lower.contains("aider") {
        return Some(Detection {
            agent: "aider".into(),
            confidence: 0.95,
            evidence: vec![format!(
                "author: {} <{}>",
                commit.author_name, commit.author_email
            )],
        });
    }
    if email_lower == "noreply@anthropic.com" || author_lower == "claude" {
        return Some(Detection {
            agent: "claude-code".into(),
            confidence: 0.95,
            evidence: vec![format!(
                "author: {} <{}>",
                commit.author_name, commit.author_email
            )],
        });
    }
    if author_lower.contains("devin-ai") || email_lower.contains("devin-ai-integration") {
        return Some(Detection {
            agent: "devin".into(),
            confidence: 0.95,
            evidence: vec![format!(
                "author: {} <{}>",
                commit.author_name, commit.author_email
            )],
        });
    }
    if author_lower.contains("openhands") || email_lower.contains("openhands") {
        return Some(Detection {
            agent: "openhands".into(),
            confidence: 0.95,
            evidence: vec![format!(
                "author: {} <{}>",
                commit.author_name, commit.author_email
            )],
        });
    }
    if author_lower.contains("cursor agent") || email_lower.contains("cursoragent") {
        return Some(Detection {
            agent: "cursor".into(),
            confidence: 0.95,
            evidence: vec![format!(
                "author: {} <{}>",
                commit.author_name, commit.author_email
            )],
        });
    }
    if author_lower.contains("google-labs-jules") || email_lower.contains("labs-jules") {
        return Some(Detection {
            agent: "jules".into(),
            confidence: 0.95,
            evidence: vec![format!(
                "author: {} <{}>",
                commit.author_name, commit.author_email
            )],
        });
    }

    // Weak message heuristics: probable, never verified.
    if msg_lower.contains("(aider)") {
        return Some(Detection {
            agent: "aider".into(),
            confidence: 0.7,
            evidence: vec!["message marker: (aider)".into()],
        });
    }
    if msg_lower.contains("generated with claude code")
        || msg_lower.contains("generated with [claude code]")
    {
        return Some(Detection {
            agent: "claude-code".into(),
            confidence: 0.8,
            evidence: vec!["message marker: Generated with Claude Code".into()],
        });
    }

    None
}

/// Evidence class of a detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceClass {
    Verified,
    Probable,
    Unknown,
}

pub fn classify(detection: Option<&Detection>) -> EvidenceClass {
    match detection {
        Some(d) if d.confidence >= VERIFIED_THRESHOLD => EvidenceClass::Verified,
        Some(d) if d.confidence >= PROBABLE_THRESHOLD => EvidenceClass::Probable,
        _ => EvidenceClass::Unknown,
    }
}

/// True for paths whose content is machine-generated rather than authored:
/// lockfiles, minified bundles, source maps, vendored trees, build output.
/// These would massively inflate "lines introduced" without measuring any
/// real authorship, so the audit excludes them everywhere.
pub fn is_generated_path(path: &str) -> bool {
    let path = path.replace('\\', "/");
    let lower = path.to_lowercase();

    // Vendored / build-output directories anywhere in the path.
    const DIRS: [&str; 7] = [
        "node_modules/",
        "vendor/",
        "vendored/",
        "third_party/",
        "dist/",
        "build/",
        "__snapshots__/",
    ];
    for d in DIRS {
        if lower.starts_with(d) || lower.contains(&format!("/{d}")) {
            return true;
        }
    }

    // Well-known lockfiles (basename match).
    let base = lower.rsplit('/').next().unwrap_or(&lower);
    const LOCKFILES: [&str; 15] = [
        "package-lock.json",
        "yarn.lock",
        "pnpm-lock.yaml",
        "cargo.lock",
        "poetry.lock",
        "uv.lock",
        "pipfile.lock",
        "gemfile.lock",
        "composer.lock",
        "go.sum",
        "flake.lock",
        "bun.lock",
        "bun.lockb",
        "deno.lock",
        "packages.lock.json",
    ];
    if LOCKFILES.contains(&base) || base.ends_with(".lockfile") {
        return true;
    }

    // Minified/generated file suffixes.
    const SUFFIXES: [&str; 8] = [
        ".min.js",
        ".min.css",
        ".map",
        ".pb.go",
        "_pb2.py",
        "_pb2_grpc.py",
        ".generated.ts",
        ".generated.go",
    ];
    SUFFIXES.iter().any(|s| base.ends_with(s))
}

/// Parse `git log --numstat` style added-line counts:
/// each entry line is `added<TAB>deleted<TAB>path`; binary files use `-`.
/// Returns total added lines (text files only, generated paths excluded).
pub fn parse_numstat_added(numstat: &str) -> u64 {
    let mut added = 0u64;
    for line in numstat.lines() {
        let mut cols = line.split('\t');
        if let (Some(a), Some(_d), Some(p)) = (cols.next(), cols.next(), cols.next()) {
            if is_generated_path(p.trim()) {
                continue;
            }
            if let Ok(n) = a.trim().parse::<u64>() {
                added += n;
            }
            // '-' (binary) parses as Err and is skipped.
        }
    }
    added
}

/// Parse `git blame --line-porcelain` output into one commit hash per line.
pub fn parse_blame_owners(porcelain: &str) -> Vec<String> {
    let mut owners = Vec::new();
    let mut expect_header = true;
    for line in porcelain.lines() {
        if expect_header {
            // Header: "<40-hex-sha> <orig_line> <final_line> [<num_lines>]"
            if let Some(hash) = line.split(' ').next() {
                if hash.len() == 40 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
                    owners.push(hash.to_string());
                    expect_header = false;
                }
            }
        } else if line.starts_with('\t') {
            // The content line terminates one porcelain record.
            expect_header = true;
        }
    }
    owners
}

/// Aggregated survival numbers for one evidence class.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SurvivalStat {
    pub commits: u64,
    pub introduced: u64,
    pub surviving: u64,
}

impl SurvivalStat {
    pub fn survival_rate(&self) -> Option<f64> {
        if self.introduced == 0 {
            None
        } else {
            Some(self.surviving as f64 / self.introduced as f64)
        }
    }
}

/// The full audit result.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct SurvivalReport {
    pub total_commits: u64,
    pub verified: SurvivalStat,
    pub probable: SurvivalStat,
    /// Per-agent verified stats.
    pub by_agent: BTreeMap<String, SurvivalStat>,
}

/// Pure aggregation: given per-commit introduced counts, detections, and the
/// blame owner of every line at HEAD, compute the survival report.
pub fn compute_survival(
    commits: &[(CommitMeta, u64)],
    detections: &HashMap<String, Detection>,
    head_owners: &[String],
) -> SurvivalReport {
    let mut report = SurvivalReport {
        total_commits: commits.len() as u64,
        ..Default::default()
    };

    // Surviving lines per commit hash.
    let mut surviving_by_hash: HashMap<&str, u64> = HashMap::new();
    for owner in head_owners {
        *surviving_by_hash.entry(owner.as_str()).or_default() += 1;
    }

    for (meta, introduced) in commits {
        let det = detections.get(&meta.hash);
        let class = classify(det);
        let surviving = surviving_by_hash
            .get(meta.hash.as_str())
            .copied()
            .unwrap_or(0)
            // A commit can only "survive" up to what it introduced; blame can
            // attribute context/moved lines, so clamp to stay honest.
            .min(*introduced);

        match class {
            EvidenceClass::Verified => {
                report.verified.commits += 1;
                report.verified.introduced += introduced;
                report.verified.surviving += surviving;
                if let Some(d) = det {
                    let entry = report.by_agent.entry(d.agent.clone()).or_default();
                    entry.commits += 1;
                    entry.introduced += introduced;
                    entry.surviving += surviving;
                }
            }
            EvidenceClass::Probable => {
                report.probable.commits += 1;
                report.probable.introduced += introduced;
                report.probable.surviving += surviving;
            }
            EvidenceClass::Unknown => {}
        }
    }

    report
}

// ---------------------------------------------------------------------------
// Git plumbing: the only external dependency of the Group-0 engine.
// ---------------------------------------------------------------------------

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .with_context(|| format!("failed to run git {:?}", args))?;
    if !out.status.success() {
        return Err(anyhow::anyhow!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Read all commits (oldest first) with hash, author, email and full message.
pub fn read_commits(dir: &Path) -> Result<Vec<CommitMeta>> {
    let raw = git(
        dir,
        &[
            "log",
            "--reverse",
            "--no-merges",
            "--pretty=format:%x00%H%x1f%an%x1f%ae%x1f%B",
        ],
    )?;
    let mut commits = Vec::new();
    for record in raw.split('\u{0}') {
        if record.trim().is_empty() {
            continue;
        }
        let mut fields = record.splitn(4, '\u{1f}');
        let hash = fields.next().unwrap_or("").trim().to_string();
        let author_name = fields.next().unwrap_or("").to_string();
        let author_email = fields.next().unwrap_or("").to_string();
        let message = fields.next().unwrap_or("").to_string();
        if hash.is_empty() {
            continue;
        }
        commits.push(CommitMeta {
            hash,
            author_name,
            author_email,
            message,
        });
    }
    Ok(commits)
}

/// Lines added by one commit (text files only).
pub fn lines_added(dir: &Path, hash: &str) -> Result<u64> {
    let raw = git(dir, &["show", "--numstat", "--format=", hash])?;
    Ok(parse_numstat_added(&raw))
}

/// Blame every tracked text file at HEAD, returning the owning commit of each
/// surviving line.
pub fn blame_head(dir: &Path) -> Result<Vec<String>> {
    let files = git(dir, &["ls-files"])?;
    let list: Vec<&str> = files
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter(|l| !is_generated_path(l))
        .collect();
    let total = list.len();
    let mut owners = Vec::new();
    for (i, file) in list.iter().enumerate() {
        if total > 200 && i % 200 == 0 {
            eprintln!("  blaming files at HEAD: {i}/{total}");
        }
        // Skip files git cannot blame (e.g. binary): tolerate per-file errors.
        if let Ok(porcelain) = git(dir, &["blame", "--line-porcelain", "HEAD", "--", file]) {
            owners.extend(parse_blame_owners(&porcelain));
        }
    }
    Ok(owners)
}

/// Full Group-0 audit of a git repository: no ledger, no hooks, no proxy.
pub fn audit_repo(dir: &Path) -> Result<SurvivalReport> {
    let commits = read_commits(dir)?;
    let mut detections: HashMap<String, Detection> = HashMap::new();
    let mut with_intro: Vec<(CommitMeta, u64)> = Vec::new();

    // Only count introduced lines for attributed commits (cheap + focused).
    let attributed: HashSet<String> = commits
        .iter()
        .filter_map(|c| detect_ai(c).map(|d| (c.hash.clone(), d)))
        .map(|(h, d)| {
            detections.insert(h.clone(), d);
            h
        })
        .collect();

    for c in commits {
        let introduced = if attributed.contains(&c.hash) {
            lines_added(dir, &c.hash)?
        } else {
            0
        };
        with_intro.push((c, introduced));
    }

    let head_owners = blame_head(dir)?;
    Ok(compute_survival(&with_intro, &detections, &head_owners))
}

// ---------------------------------------------------------------------------
// Tests — written first: they define the behavior of the audit engine.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(hash: &str, author: &str, email: &str, message: &str) -> CommitMeta {
        CommitMeta {
            hash: hash.into(),
            author_name: author.into(),
            author_email: email.into(),
            message: message.into(),
        }
    }

    // -- detection ----------------------------------------------------------

    #[test]
    fn detects_claude_code_trailer_as_verified() {
        let c = meta(
            "a".repeat(40).as_str(),
            "Tarik",
            "tarik@example.com",
            "fix auth race\n\nCo-Authored-By: Claude <noreply@anthropic.com>",
        );
        let d = detect_ai(&c).expect("should detect");
        assert_eq!(d.agent, "claude-code");
        assert_eq!(classify(Some(&d)), EvidenceClass::Verified);
        assert!(d.evidence[0].contains("trailer"));
    }

    #[test]
    fn detects_copilot_trailer_as_verified() {
        let c = meta(
            "b".repeat(40).as_str(),
            "Dev",
            "dev@example.com",
            "add tests\n\nCo-authored-by: GitHub Copilot <copilot@github.com>",
        );
        let d = detect_ai(&c).expect("should detect");
        assert_eq!(d.agent, "github-copilot");
        assert_eq!(classify(Some(&d)), EvidenceClass::Verified);
    }

    #[test]
    fn trailer_detection_is_case_insensitive() {
        let c = meta(
            "c".repeat(40).as_str(),
            "Dev",
            "dev@example.com",
            "refactor\n\nCO-AUTHORED-BY: CLAUDE <noreply@anthropic.com>",
        );
        assert!(detect_ai(&c).is_some());
    }

    #[test]
    fn detects_aider_author_as_verified() {
        let c = meta(
            "d".repeat(40).as_str(),
            "Tarik (aider)",
            "tarik@example.com",
            "implement retry",
        );
        let d = detect_ai(&c).expect("should detect");
        assert_eq!(d.agent, "aider");
        assert_eq!(classify(Some(&d)), EvidenceClass::Verified);
    }

    #[test]
    fn detects_aider_message_marker_as_probable_only() {
        let c = meta(
            "e".repeat(40).as_str(),
            "Tarik",
            "tarik@example.com",
            "fix parser (aider)",
        );
        let d = detect_ai(&c).expect("should detect");
        assert_eq!(d.agent, "aider");
        assert_eq!(classify(Some(&d)), EvidenceClass::Probable);
    }

    #[test]
    fn human_commit_is_unknown() {
        let c = meta(
            "f".repeat(40).as_str(),
            "Tarik",
            "tarik@example.com",
            "hand-written fix, no AI involved",
        );
        assert!(detect_ai(&c).is_none());
        assert_eq!(classify(None), EvidenceClass::Unknown);
    }

    #[test]
    fn detects_new_agent_trailers_as_verified() {
        for (trailer, agent) in [
            ("Co-Authored-By: Codex <codex@openai.com>", "openai-codex"),
            (
                "Co-Authored-By: ChatGPT <chatgpt@openai.com>",
                "openai-codex",
            ),
            ("Co-Authored-By: Gemini <gemini@google.com>", "gemini"),
            (
                "Co-Authored-By: openhands <openhands@all-hands.dev>",
                "openhands",
            ),
            (
                "Co-Authored-By: Devin AI <devin-ai-integration[bot]@users.noreply.github.com>",
                "devin",
            ),
            ("Co-Authored-By: Jules <jules@google.com>", "jules"),
        ] {
            let c = meta(
                "9".repeat(40).as_str(),
                "Dev",
                "dev@example.com",
                &format!("change\n\n{trailer}"),
            );
            let d = detect_ai(&c).unwrap_or_else(|| panic!("should detect {trailer}"));
            assert_eq!(d.agent, agent, "trailer: {trailer}");
            assert_eq!(classify(Some(&d)), EvidenceClass::Verified);
        }
    }

    #[test]
    fn detects_new_bot_authors_as_verified() {
        for (author, email, agent) in [
            ("openhands", "openhands@all-hands.dev", "openhands"),
            ("Cursor Agent", "cursoragent@cursor.com", "cursor"),
            (
                "google-labs-jules[bot]",
                "12345+google-labs-jules[bot]@users.noreply.github.com",
                "jules",
            ),
        ] {
            let c = meta("8".repeat(40).as_str(), author, email, "routine change");
            let d = detect_ai(&c).unwrap_or_else(|| panic!("should detect {author}"));
            assert_eq!(d.agent, agent, "author: {author}");
            assert_eq!(classify(Some(&d)), EvidenceClass::Verified);
        }
    }

    #[test]
    fn claude_code_message_marker_is_probable_only() {
        let c = meta(
            "7".repeat(40).as_str(),
            "Tarik",
            "tarik@example.com",
            "fix parser\n\n\u{1f916} Generated with [Claude Code](https://claude.ai/code)",
        );
        let d = detect_ai(&c).expect("should detect");
        assert_eq!(d.agent, "claude-code");
        assert_eq!(classify(Some(&d)), EvidenceClass::Probable);
    }

    #[test]
    fn coauthor_human_is_not_misdetected() {
        // A human co-author must not trigger detection.
        let c = meta(
            "1".repeat(40).as_str(),
            "Tarik",
            "tarik@example.com",
            "pair session\n\nCo-Authored-By: Alice <alice@example.com>",
        );
        assert!(detect_ai(&c).is_none());
    }

    // -- numstat parsing ------------------------------------------------------

    #[test]
    fn numstat_sums_added_lines_and_skips_binary() {
        let numstat = "10\t2\tsrc/auth.rs\n3\t0\tsrc/lib.rs\n-\t-\tassets/logo.png\n";
        assert_eq!(parse_numstat_added(numstat), 13);
    }

    #[test]
    fn numstat_empty_input_is_zero() {
        assert_eq!(parse_numstat_added(""), 0);
    }

    // -- generated-path exclusion ----------------------------------------------

    #[test]
    fn generated_paths_are_detected() {
        for p in [
            "package-lock.json",
            "web/package-lock.json",
            "yarn.lock",
            "Cargo.lock",
            "uv.lock",
            "go.sum",
            "gradle/deps.lockfile",
            "node_modules/react/index.js",
            "src/vendor/lib.c",
            "third_party/proto/x.py",
            "dist/bundle.js",
            "build/out.o",
            "app/__snapshots__/ui.snap",
            "assets/app.min.js",
            "styles/app.min.css",
            "js/app.js.map",
            "api/service.pb.go",
            "gen/thing_pb2.py",
        ] {
            assert!(is_generated_path(p), "{p} should be generated");
        }
    }

    #[test]
    fn authored_paths_are_not_generated() {
        for p in [
            "src/main.rs",
            "auth.py",
            "docs/lock-design.md",
            "src/locker.rs",
            "distributed/map.rs",
            "builder/build_config.rs",
            "app.js",
        ] {
            assert!(!is_generated_path(p), "{p} should NOT be generated");
        }
    }

    #[test]
    fn numstat_excludes_generated_files() {
        let numstat = "10\t2\tsrc/auth.rs\n5000\t0\tpackage-lock.json\n300\t0\tdist/bundle.js\n3\t0\tsrc/lib.rs\n";
        assert_eq!(parse_numstat_added(numstat), 13);
    }

    // -- blame parsing --------------------------------------------------------

    #[test]
    fn blame_porcelain_extracts_one_owner_per_line() {
        let a = "a".repeat(40);
        let b = "b".repeat(40);
        // Two porcelain records: headers + metadata + tab-prefixed content.
        let porcelain = format!(
            "{a} 1 1 1\nauthor Tarik\nfilename src/x.rs\n\tline one\n{b} 2 2 1\nauthor Claude\nfilename src/x.rs\n\tline two\n"
        );
        let owners = parse_blame_owners(&porcelain);
        assert_eq!(owners, vec![a, b]);
    }

    #[test]
    fn blame_metadata_lines_are_not_mistaken_for_headers() {
        let a = "a".repeat(40);
        let porcelain = format!(
            "{a} 1 1 1\nauthor-mail <x@y.z>\nsummary deadbeef in text\nfilename f\n\tcontent\n"
        );
        assert_eq!(parse_blame_owners(&porcelain).len(), 1);
    }

    // -- survival aggregation -------------------------------------------------

    fn detection(agent: &str, confidence: f64) -> Detection {
        Detection {
            agent: agent.into(),
            confidence,
            evidence: vec![],
        }
    }

    #[test]
    fn survival_counts_only_attributed_classes() {
        let ai = meta(&"a".repeat(40), "x", "x@x", "Co-Authored-By: Claude <n@a>");
        let human = meta(&"b".repeat(40), "x", "x@x", "manual");

        let mut detections = HashMap::new();
        detections.insert(ai.hash.clone(), detection("claude-code", 1.0));

        // AI introduced 5 lines, 3 survive; human lines never counted.
        let commits = vec![(ai.clone(), 5u64), (human.clone(), 100u64)];
        let head_owners: Vec<String> = std::iter::repeat_n(ai.hash.clone(), 3)
            .chain(std::iter::repeat_n(human.hash.clone(), 50))
            .collect();

        let report = compute_survival(&commits, &detections, &head_owners);
        assert_eq!(report.total_commits, 2);
        assert_eq!(report.verified.commits, 1);
        assert_eq!(report.verified.introduced, 5);
        assert_eq!(report.verified.surviving, 3);
        assert_eq!(report.verified.survival_rate(), Some(0.6));
        // Human commit contributes nothing to any attributed class.
        assert_eq!(report.probable, SurvivalStat::default());
        // Per-agent aggregation present.
        assert_eq!(report.by_agent["claude-code"].surviving, 3);
    }

    #[test]
    fn probable_and_verified_are_kept_separate() {
        let v = meta(&"a".repeat(40), "x", "x@x", "m");
        let p = meta(&"b".repeat(40), "x", "x@x", "m");
        let mut detections = HashMap::new();
        detections.insert(v.hash.clone(), detection("claude-code", 1.0));
        detections.insert(p.hash.clone(), detection("aider", 0.7));

        let commits = vec![(v.clone(), 10u64), (p.clone(), 10u64)];
        let owners: Vec<String> = std::iter::repeat_n(v.hash.clone(), 4)
            .chain(std::iter::repeat_n(p.hash.clone(), 9))
            .collect();

        let report = compute_survival(&commits, &detections, &owners);
        assert_eq!(report.verified.surviving, 4);
        assert_eq!(report.probable.surviving, 9);
        // Probable agents never leak into the per-agent verified table.
        assert!(!report.by_agent.contains_key("aider"));
    }

    #[test]
    fn surviving_lines_are_clamped_to_introduced() {
        // Blame can attribute moved/context lines; never report >100% survival.
        let c = meta(&"a".repeat(40), "x", "x@x", "m");
        let mut detections = HashMap::new();
        detections.insert(c.hash.clone(), detection("claude-code", 1.0));
        let commits = vec![(c.clone(), 2u64)];
        let owners: Vec<String> = std::iter::repeat_n(c.hash.clone(), 7).collect();

        let report = compute_survival(&commits, &detections, &owners);
        assert_eq!(report.verified.surviving, 2);
        assert_eq!(report.verified.survival_rate(), Some(1.0));
    }

    #[test]
    fn survival_rate_is_none_when_nothing_introduced() {
        assert_eq!(SurvivalStat::default().survival_rate(), None);
    }

    // -- end-to-end on a real synthetic git repo -------------------------------

    fn run_git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "Tarik")
            .env("GIT_AUTHOR_EMAIL", "tarik@example.com")
            .env("GIT_COMMITTER_NAME", "Tarik")
            .env("GIT_COMMITTER_EMAIL", "tarik@example.com")
            .status()
            .expect("git must be installed")
            .success();
        assert!(ok, "git {:?} failed", args);
    }

    #[test]
    fn audits_a_synthetic_repo_end_to_end() {
        let tmp = std::env::temp_dir().join(format!("causari-audit-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        run_git(&tmp, &["init", "-q", "-b", "main"]);

        // Commit 1 (human): baseline.
        std::fs::write(tmp.join("main.py"), "print('hello')\n").unwrap();
        run_git(&tmp, &["add", "."]);
        run_git(&tmp, &["commit", "-q", "-m", "initial scaffold"]);

        // Commit 2 (AI, Claude trailer): adds 3 lines.
        std::fs::write(
            tmp.join("auth.py"),
            "def refresh(user):\n    token = rotate(user)\n    return token\n",
        )
        .unwrap();
        run_git(&tmp, &["add", "."]);
        run_git(
            &tmp,
            &[
                "commit",
                "-q",
                "-m",
                "add token refresh\n\nCo-Authored-By: Claude <noreply@anthropic.com>",
            ],
        );

        // Commit 3 (human): deletes one AI line -> 2 of 3 AI lines survive.
        std::fs::write(
            tmp.join("auth.py"),
            "def refresh(user):\n    return rotate(user)\n",
        )
        .unwrap();
        run_git(&tmp, &["add", "."]);
        run_git(&tmp, &["commit", "-q", "-m", "simplify refresh by hand"]);

        let report = audit_repo(&tmp).expect("audit must succeed");

        assert_eq!(report.total_commits, 3);
        assert_eq!(report.verified.commits, 1);
        assert_eq!(report.verified.introduced, 3);
        // "def refresh(user):" survives; "return token"/"token = rotate" were
        // replaced. Exactly 1 original AI line remains attributable at HEAD.
        assert_eq!(report.verified.surviving, 1);
        assert!(report.by_agent.contains_key("claude-code"));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
