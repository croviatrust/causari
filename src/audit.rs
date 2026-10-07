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
//! authorship signal is UNKNOWN and never enters the headline figures; since
//! method v3 those commits form the repository's own baseline (`Baseline`),
//! so a VERIFIED rate is read next to the untagged rate in the matched age windows.

use anyhow::{Context, Result};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// How sure we are that a commit's metadata tags it as AI-authored, and why.
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
    /// Git notes attached under `refs/notes/ai` (git-ai authorship logs),
    /// empty when absent.
    pub notes: String,
    /// Committer timestamp, seconds since the Unix epoch: when the commit
    /// entered this history, which is what a line's age is counted from.
    pub committed_at: i64,
}

/// Canonical name of a vendor whose product name appears anywhere in the
/// lowercased hint. Used for values the author chose to be about a tool
/// (trailer values, tool ids), never for people's names.
fn known_agent(h: &str) -> Option<&'static str> {
    if h.contains("claude") || h.contains("anthropic") {
        Some("claude-code")
    } else if h.contains("copilot") {
        Some("github-copilot")
    } else if h.contains("cursor") {
        Some("cursor")
    } else if h.contains("aider") {
        Some("aider")
    } else if h.contains("codex")
        || h.contains("chatgpt")
        || h.contains("gpt")
        || h.contains("openai")
    {
        Some("openai-codex")
    } else if h.contains("gemini") {
        Some("gemini")
    } else if h.contains("devin") {
        Some("devin")
    } else if h.contains("openhands") {
        Some("openhands")
    } else if h.contains("jules") {
        Some("jules")
    } else {
        None
    }
}

fn agent_slug_chars(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'
}

/// Map a free-form agent/model hint (trailer value, tool name, `~handle`) to
/// Causari's canonical agent names. Unknown hints keep their first token.
pub fn canonical_agent(hint: &str) -> String {
    let h = hint.trim().trim_start_matches('~').to_lowercase();
    if let Some(agent) = known_agent(&h) {
        return agent.into();
    }
    let token: String = h
        .split(|c: char| c.is_whitespace() || c == '<' || c == '(' || c == '/')
        .next()
        .unwrap_or("ai")
        .chars()
        .filter(|c| agent_slug_chars(*c))
        .collect();
    if token.is_empty() { "ai".into() } else { token }
}

/// Agent named by an `Assisted-by:` trailer (Linux kernel, Fedora, LLVM,
/// OpenTelemetry convention). The value is a tool name, optionally followed
/// by a model in parentheses or after a colon: `Claude Code (claude-sonnet-4)`,
/// `Claude:claude-3-5-sonnet`, `Cursor`. The tool's words, lowercased and
/// hyphen-joined, form the agent; known vendors map to their canonical name.
pub fn assisted_by_agent(value: &str) -> String {
    let tool = value
        .split(['(', ':', '<', ',', '/', '['])
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if let Some(agent) = known_agent(&tool) {
        return agent.into();
    }
    let words: Vec<String> = tool
        .split_whitespace()
        .map(|w| {
            w.chars()
                .filter(|c| agent_slug_chars(*c))
                .collect::<String>()
        })
        .filter(|w| !w.is_empty())
        // "v2", "2.1": a version suffix is not part of the tool's name.
        .take_while(|w| !w.starts_with(|c: char| c.is_ascii_digit()) && !is_version_token(w))
        .collect();
    if words.is_empty() {
        "ai".into()
    } else {
        words.join("-")
    }
}

fn is_version_token(w: &str) -> bool {
    let mut chars = w.chars();
    chars.next() == Some('v') && chars.all(|c| c.is_ascii_digit() || c == '.')
}

/// One trailer of a commit message, key lowercased.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trailer {
    pub key: String,
    pub value: String,
    /// The trailer's first line as written, for evidence strings.
    pub line: String,
}

/// `Token: value` with token `[A-Za-z0-9-]+`; the separator must be followed
/// by whitespace or end the line, so `https://…` is prose, not a trailer.
fn split_trailer_line(line: &str) -> Option<(&str, &str)> {
    let (token, rest) = line.split_once(':')?;
    let token = token.trim_end();
    if token.is_empty()
        || !token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        || !(rest.is_empty() || rest.starts_with([' ', '\t']))
    {
        return None;
    }
    Some((token, rest.trim()))
}

/// Trailers of a commit message under git's own rules (`git
/// interpret-trailers`): only the last paragraph is examined and the subject
/// paragraph never is. Every line must be a trailer or an indented
/// continuation of one, unless the paragraph carries a git-generated
/// trailer (`Signed-off-by`, cherry-pick marker) and is at least one quarter
/// trailers — then its non-trailer lines are skipped. `Executed-By: CI
/// pipeline` in body prose is therefore not a trailer.
pub fn parse_trailers(message: &str) -> Vec<Trailer> {
    let lines: Vec<&str> = message.lines().map(|l| l.trim_end_matches('\r')).collect();
    let end = lines
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .map(|i| i + 1)
        .unwrap_or(0);
    // Paragraph boundary: the last blank line. None means the message is a
    // single paragraph, i.e. subject only.
    let Some(blank) = lines[..end].iter().rposition(|l| l.trim().is_empty()) else {
        return Vec::new();
    };
    let block = &lines[blank + 1..end];

    let mut trailers: Vec<Trailer> = Vec::new();
    let mut non_trailer_lines = 0usize;
    let mut git_generated = false;
    for line in block {
        if line.starts_with([' ', '\t']) {
            match trailers.last_mut() {
                Some(t) => {
                    t.value.push(' ');
                    t.value.push_str(line.trim());
                }
                None => non_trailer_lines += 1,
            }
            continue;
        }
        if line.starts_with("(cherry picked from commit ") {
            git_generated = true;
            non_trailer_lines += 1;
            continue;
        }
        match split_trailer_line(line) {
            Some((key, value)) => {
                let key = key.to_lowercase();
                git_generated |= key == "signed-off-by";
                trailers.push(Trailer {
                    key,
                    value: value.to_string(),
                    line: line.trim().to_string(),
                });
            }
            None => non_trailer_lines += 1,
        }
    }

    let is_block = !trailers.is_empty()
        && (non_trailer_lines == 0 || (git_generated && trailers.len() * 3 >= non_trailer_lines));
    if is_block { trailers } else { Vec::new() }
}

/// True when `word` occurs in `text` delimited by non-alphanumerics: "aider"
/// matches "aider <aider@aider.chat>" but not "raider"; "cursor" matches
/// "@cursor.com" but not "precursor".
fn contains_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(i, _)| {
        let before = text[..i]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        let after = text[i + word.len()..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric());
        before && after
    })
}

/// Agent named in a git-ai authorship log (`refs/notes/ai`, schema
/// `authorship/x.y.z`): the metadata JSON after the `---` divider carries
/// `sessions.*.agent_id.tool` or `prompts.*.agent_id.tool`.
///
/// `schema_version` proves only that the object uses a git-ai schema. A
/// human-only note, an empty tool, and a note with no tool are not AI
/// evidence: this function returns `None` and detection continues. A
/// non-empty tool is qualifying metadata. Sessions are scanned before
/// prompts. Within one object, `serde_json` iterates keys in lexicographic
/// order, so two named tools yield one deterministic agent. That choice is
/// not a finding about which tool wrote the commit.
fn git_ai_agent(notes: &str) -> Option<String> {
    if !notes.contains("schema_version") {
        return None;
    }
    let json = notes.split_once("\n---\n").map(|(_, j)| j).unwrap_or(notes);
    let v: serde_json::Value = serde_json::from_str(json.trim()).ok()?;
    for map in ["sessions", "prompts"] {
        if let Some(obj) = v.get(map).and_then(|m| m.as_object()) {
            for rec in obj.values() {
                let Some(tool) = rec
                    .get("agent_id")
                    .and_then(|a| a.get("tool"))
                    .and_then(|t| t.as_str())
                else {
                    continue;
                };
                if tool.trim().is_empty() {
                    continue;
                }
                return Some(canonical_agent(tool));
            }
        }
    }
    None
}

/// Classify a commit as AI-tagged (or not) from its metadata alone: the
/// tag is what the metadata says, not a judgement about who typed the code.
///
/// Detectors are ordered strongest-first; the first match wins. Trailers are
/// read from the git trailer block only (see [`parse_trailers`]). Signals:
/// - git-ai note naming a non-empty tool       -> that tool, verified (1.0)
/// - `Drafted-With`, `AI-Agent`, `Assisted-by`… -> named tool, verified (1.0)
/// - `Co-Authored-By: Claude`                  -> claude-code, verified (1.0)
/// - `Co-Authored-By: ... Copilot`             -> github-copilot, verified (1.0)
/// - aider author/committer marker             -> aider, verified (0.95)
/// - known bot authors (`copilot-swe-agent`…)  -> named bot, verified (0.95)
/// - `(aider)` suffix in message               -> aider, probable (0.7)
pub fn detect_ai(commit: &CommitMeta) -> Option<Detection> {
    let msg_lower = commit.message.to_lowercase();
    let author_lower = commit.author_name.to_lowercase();
    let email_lower = commit.author_email.to_lowercase();

    // git-ai note: a named tool is commit-level metadata, not line attribution.
    if let Some(agent) = git_ai_agent(&commit.notes) {
        return Some(Detection {
            agent,
            confidence: 1.0,
            evidence: vec!["git-ai authorship note (refs/notes/ai)".into()],
        });
    }

    // Only the trailer block counts: `Key: value` in body prose is prose.
    let trailers = parse_trailers(&commit.message);

    // Structured provenance trailers from emerging standards:
    // IETF draft-morrison identity-attributed commits (`Drafted-With`,
    // `Executed-By`), the `AI-*` trailer family (`AI-Model`, `AI-Agent`,
    // `AI-Session-ID`, `AI-Provenance`) and `Assisted-by` (Linux kernel,
    // Fedora, LLVM, OpenTelemetry).
    let mut ai_marker: Option<&Trailer> = None;
    for t in &trailers {
        if t.value.is_empty() || is_negative_disclosure(&t.value) {
            continue;
        }
        match t.key.as_str() {
            "drafted-with" | "executed-by" | "ai-model" | "ai-agent" | "ai-tool" => {
                return Some(Detection {
                    agent: canonical_agent(&t.value),
                    confidence: 1.0,
                    evidence: vec![format!("trailer: {}", t.line)],
                });
            }
            "assisted-by" => {
                return Some(Detection {
                    agent: assisted_by_agent(&t.value),
                    confidence: 1.0,
                    evidence: vec![format!("trailer: {}", t.line)],
                });
            }
            "ai-session-id" | "ai-provenance" | "ai-generated" | "ai-assisted"
                if ai_marker.is_none() =>
            {
                ai_marker = Some(t);
            }
            _ => {}
        }
    }
    if let Some(marker) = ai_marker {
        return Some(Detection {
            agent: "ai".into(),
            confidence: 1.0,
            evidence: vec![format!("trailer: {}", marker.line)],
        });
    }

    // Co-author trailers naming an agent identity.
    for t in trailers.iter().filter(|t| t.key == "co-authored-by") {
        if let Some(agent) = coauthor_agent(&t.value.to_lowercase()) {
            return Some(Detection {
                agent: agent.into(),
                confidence: 1.0,
                evidence: vec![format!("trailer: {}", t.line)],
            });
        }
    }

    // Author-identity detection.
    if author_lower.contains("(aider)") || contains_word(&email_lower, "aider") {
        return Some(Detection {
            agent: "aider".into(),
            confidence: 0.95,
            evidence: vec![format!(
                "author: {} <{}>",
                commit.author_name, commit.author_email
            )],
        });
    }
    // The address is the bot identity. A person whose git author name is
    // "Claude" and whose email is their own is not this signal: the method
    // table names `noreply@anthropic.com`, not the display name.
    if email_lower == "noreply@anthropic.com" {
        return Some(Detection {
            agent: "claude-code".into(),
            confidence: 0.95,
            evidence: vec![format!(
                "author: {} <{}>",
                commit.author_name, commit.author_email
            )],
        });
    }
    // GitHub Copilot coding agent commits as its own bot account.
    if author_lower.contains("copilot-swe-agent") || email_lower.contains("copilot-swe-agent") {
        return Some(Detection {
            agent: "copilot".into(),
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

/// Values of an `AI-*` / `Drafted-With` / `Executed-By` trailer that
/// explicitly deny AI involvement. Projects with a disclosure policy write
/// `AI-Assisted: no` on every human commit; that must never count as AI.
fn is_negative_disclosure(value: &str) -> bool {
    let v = value.trim().trim_end_matches('.').to_lowercase();
    matches!(
        v.as_str(),
        "no" | "none" | "false" | "0" | "n/a" | "na" | "human" | "manual" | "not used"
    )
}

/// Agent named by a lowercased `Co-authored-by` value (`name <email>`), if
/// any. Product names are matched as whole words so "raider" and "precursor"
/// are people. Names that double as human names (Devin, Jules, Gemini,
/// Cursor, Claude) additionally need a vendor or bot address, or — for
/// Claude — a display name that is the tool's ("Claude", "Claude Code",
/// "Claude Opus 4"), not a person's.
fn coauthor_agent(rest: &str) -> Option<&'static str> {
    let name = rest.split('<').next().unwrap_or("").trim();
    let vendor = coauthor_is_vendor_identity(rest);
    let claude_name = name == "claude"
        || [
            "claude code",
            "claude opus",
            "claude sonnet",
            "claude haiku",
        ]
        .iter()
        .any(|p| name.starts_with(p));
    if rest.contains("noreply@anthropic.com") || claude_name {
        return Some("claude-code");
    }
    if contains_word(rest, "copilot") {
        return Some("github-copilot");
    }
    if vendor && contains_word(rest, "cursor") {
        return Some("cursor");
    }
    if contains_word(rest, "aider") {
        return Some("aider");
    }
    if contains_word(rest, "codex") || contains_word(rest, "chatgpt") {
        return Some("openai-codex");
    }
    if vendor && contains_word(rest, "gemini") {
        return Some("gemini");
    }
    if contains_word(rest, "openhands") {
        return Some("openhands");
    }
    if vendor && contains_word(rest, "devin") {
        return Some("devin");
    }
    if vendor && contains_word(rest, "jules") {
        return Some("jules");
    }
    None
}

/// True when a lowercased `Co-authored-by` value (`name <email>`) points at
/// an AI vendor or a GitHub bot account rather than a person. Used to gate
/// agents whose product names double as human names.
fn coauthor_is_vendor_identity(rest: &str) -> bool {
    let email = rest
        .rsplit_once('<')
        .map(|(_, e)| e.trim_end_matches('>').trim())
        .unwrap_or("");
    if email.contains("[bot]") || rest.contains("[bot]") {
        return true;
    }
    const BOT_IDS: [&str; 4] = [
        "devin-ai-integration",
        "labs-jules",
        "cursoragent",
        "gemini-code-assist",
    ];
    if BOT_IDS.iter().any(|id| email.contains(id)) {
        return true;
    }
    const VENDOR_DOMAINS: [&str; 8] = [
        "@google.com",
        "@cursor.com",
        "@cursor.sh",
        "@cognition.ai",
        "@cognition-labs.com",
        "@devin.ai",
        "@openai.com",
        "@anthropic.com",
    ];
    VENDOR_DOMAINS.iter().any(|d| email.ends_with(d))
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

/// Version of the measurement method that produced a report. Bumped when a
/// number computed from the same repository can change, or when the report
/// gains a figure a reader must not look for in older results. v3 adds the
/// untagged baseline of the same repository (`baseline`); every v2 figure is
/// computed exactly as before and can be compared across the two versions.
/// v4 keeps that baseline and every other v3 rule except one: a git-ai note
/// is AI evidence only when it names a non-empty tool. Method v3 counted any
/// parseable note, including a schema-only or human-only note, as VERIFIED
/// agent `ai`. Reports published under v3 stay v3.
pub const METHOD_VERSION: &str = "v4";

/// Below this many VERIFIED commits a ratio is reported but flagged: one
/// commit can dominate it.
pub const SAMPLE_FLOOR: u64 = 5;

/// Per-commit weight cap rule for `capped_survival_rate`: a commit weighs at
/// most the 95th percentile (nearest rank) of per-commit introduced line
/// counts within its group, and never more than this many lines. With fewer
/// than 20 commits the percentile is the largest commit, so only the
/// absolute ceiling bites.
pub const CAP_PERCENTILE: f64 = 0.95;
pub const CAP_CEILING_LINES: u64 = 10_000;

/// Aggregated survival numbers for one evidence class or one agent.
///
/// `commits`, `introduced` and `surviving` are plain sums. The per-commit
/// pairs behind them are kept so the report can also state figures that one
/// bulk commit cannot dominate: the median per-commit rate, a weight-capped
/// rate and the share of the largest commit.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SurvivalStat {
    pub commits: u64,
    pub introduced: u64,
    pub surviving: u64,
    /// `(introduced, surviving)` of every commit in this group.
    pub per_commit: Vec<(u64, u64)>,
}

impl SurvivalStat {
    pub fn record(&mut self, introduced: u64, surviving: u64) {
        self.commits += 1;
        self.introduced += introduced;
        self.surviving += surviving;
        self.per_commit.push((introduced, surviving));
    }

    /// Line-weighted ratio: Σ surviving / Σ introduced.
    pub fn survival_rate(&self) -> Option<f64> {
        if self.introduced == 0 {
            None
        } else {
            Some(self.surviving as f64 / self.introduced as f64)
        }
    }

    fn measured(&self) -> impl Iterator<Item = (u64, u64)> + '_ {
        self.per_commit.iter().copied().filter(|(i, _)| *i > 0)
    }

    /// Median of per-commit survival rates over commits that introduced at
    /// least one line.
    pub fn median_survival(&self) -> Option<f64> {
        let mut rates: Vec<f64> = self.measured().map(|(i, s)| s as f64 / i as f64).collect();
        if rates.is_empty() {
            return None;
        }
        rates.sort_by(|a, b| a.total_cmp(b));
        let n = rates.len();
        Some(if n % 2 == 1 {
            rates[n / 2]
        } else {
            (rates[n / 2 - 1] + rates[n / 2]) / 2.0
        })
    }

    /// The per-commit weight cap in lines (see [`CAP_PERCENTILE`]).
    pub fn cap_lines(&self) -> Option<u64> {
        let mut sizes: Vec<u64> = self.measured().map(|(i, _)| i).collect();
        if sizes.is_empty() {
            return None;
        }
        sizes.sort_unstable();
        let rank = ((sizes.len() as f64 * CAP_PERCENTILE).ceil() as usize).clamp(1, sizes.len());
        Some(sizes[rank - 1].min(CAP_CEILING_LINES))
    }

    /// Line-weighted ratio where no commit weighs more than [`cap_lines`]:
    /// a commit above the cap contributes `cap × its own rate`.
    ///
    /// [`cap_lines`]: SurvivalStat::cap_lines
    pub fn capped_survival_rate(&self) -> Option<f64> {
        let cap = self.cap_lines()? as f64;
        let (num, den) = self.measured().fold((0.0, 0.0), |(num, den), (i, s)| {
            let weight = (i as f64).min(cap);
            (num + weight * (s as f64 / i as f64), den + weight)
        });
        if den == 0.0 { None } else { Some(num / den) }
    }

    /// Fraction of introduced lines that come from the single largest commit.
    pub fn largest_commit_share(&self) -> Option<f64> {
        if self.introduced == 0 {
            return None;
        }
        let largest = self.measured().map(|(i, _)| i).max().unwrap_or(0);
        Some(largest as f64 / self.introduced as f64)
    }

    /// Whether one commit holds at least half of the introduced lines: the
    /// row then measures that commit rather than the group.
    pub fn dominated_by_one_commit(&self) -> bool {
        self.largest_commit_share().is_some_and(|s| s >= 0.5)
    }
}

/// The serialized shape of a `SurvivalStat`: sums plus derived figures, so
/// JSON, snapshots and the data workflow all see the same set of fields.
#[derive(serde::Serialize)]
struct SurvivalStatView {
    commits: u64,
    introduced: u64,
    surviving: u64,
    survival_rate: Option<f64>,
    median_survival: Option<f64>,
    capped_survival_rate: Option<f64>,
    cap_lines: Option<u64>,
    largest_commit_share: Option<f64>,
}

impl serde::Serialize for SurvivalStat {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        SurvivalStatView {
            commits: self.commits,
            introduced: self.introduced,
            surviving: self.surviving,
            survival_rate: self.survival_rate(),
            median_survival: self.median_survival(),
            capped_survival_rate: self.capped_survival_rate(),
            cap_lines: self.cap_lines(),
            largest_commit_share: self.largest_commit_share(),
        }
        .serialize(serializer)
    }
}

/// What the measurement covered and how: printed next to every number so a
/// reader can tell two runs apart before comparing them.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Coverage {
    pub method: &'static str,
    pub blame_flags: Vec<&'static str>,
    /// A usable `.git-blame-ignore-revs` was passed to blame.
    pub ignore_revs_file: bool,
    /// The repository is a shallow clone: introduced counts are truncated
    /// and blame stops at the shallow boundary.
    pub shallow: bool,
    pub sample_floor: u64,
    /// Fewer VERIFIED commits than `sample_floor`.
    pub small_sample: bool,
}

impl Default for Coverage {
    fn default() -> Self {
        Coverage {
            method: METHOD_VERSION,
            blame_flags: BLAME_FLAGS.to_vec(),
            ignore_revs_file: false,
            shallow: false,
            sample_floor: SAMPLE_FLOOR,
            small_sample: true,
        }
    }
}

/// Upper edges, in days, of the age buckets of the by-age table (method
/// v3). A line's age is the time from its commit's committer date to the
/// committer date of HEAD, so the table depends only on the repository, not
/// on when the audit ran. The last bucket is open-ended.
pub const AGE_EDGES_DAYS: [u64; 5] = [30, 90, 180, 365, 730];

/// One row of the by-age table: VERIFIED lines and untagged lines that
/// entered the repository in one age window. A window is a bucket of commit
/// age, not a single timestamp.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize)]
pub struct AgeBucket {
    pub from_days: u64,
    /// `None` for the open-ended last bucket.
    pub to_days: Option<u64>,
    pub tagged: SurvivalStat,
    pub untagged: SurvivalStat,
}

impl AgeBucket {
    pub fn label(&self) -> String {
        match self.to_days {
            Some(to) => format!("{}-{} d", self.from_days, to),
            None => format!("{}+ d", self.from_days),
        }
    }

    /// Both cohorts have at least the sample floor in this window, so their
    /// rates can be put side by side.
    pub fn comparable(&self) -> bool {
        self.tagged.commits >= SAMPLE_FLOOR
            && self.untagged.commits >= SAMPLE_FLOOR
            && self.tagged.introduced > 0
            && self.untagged.introduced > 0
    }
}

/// VERIFIED survival against the untagged survival of the same repository,
/// inside matched age windows. A window is one bucket of commit age, not a
/// single timestamp. The untagged rate is re-weighted to the age mix of the
/// VERIFIED lines in the comparable buckets (direct standardisation), so
/// "AI code is newer" cannot by itself produce a gap.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AgeMatched {
    /// Line-weighted VERIFIED survival over the comparable buckets.
    pub tagged_rate: f64,
    /// Untagged survival weighted by the VERIFIED introduced lines of each
    /// comparable bucket.
    pub untagged_rate: f64,
    /// `tagged_rate - untagged_rate`, in rate units (0.05 = five points).
    pub gap: f64,
    pub buckets_used: usize,
    /// Share of all VERIFIED introduced lines that lie inside the matched
    /// windows. Below 1.0, some VERIFIED lines sit in windows that are not
    /// comparable, and those lines are not in `gap`.
    pub tagged_lines_covered: f64,
}

/// The oldest commit that still owns a line at HEAD, any class, and what
/// lies behind it. When many commits predate it, the repository was cleared
/// or rewritten at some point: nothing from before that date survives,
/// tagged or not, and every survival figure over those commits measures
/// the rewrite, not the code.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize)]
pub struct OldestSurviving {
    /// Committer date, `YYYY-MM-DD`.
    pub date: String,
    pub age_days: u64,
    pub commits_before: u64,
    pub introduced_before: u64,
    pub tagged_commits_before: u64,
    pub tagged_introduced_before: u64,
}

/// Method v3: the same repository's untagged lines as the baseline for its
/// VERIFIED lines. Costs nothing extra: `git log --numstat` already walks
/// every commit and `git blame` already names the owner of every line.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize)]
pub struct Baseline {
    /// Commits with no machine-readable AI signal: human-written, inline-
    /// completed and untagged-agent code alike. PROBABLE commits are in
    /// neither cohort.
    pub untagged: SurvivalStat,
    pub by_age: Vec<AgeBucket>,
    pub age_matched: Option<AgeMatched>,
    pub oldest_surviving: Option<OldestSurviving>,
}

impl Baseline {
    fn bucket_index(age_days: u64) -> usize {
        AGE_EDGES_DAYS
            .iter()
            .position(|edge| age_days < *edge)
            .unwrap_or(AGE_EDGES_DAYS.len())
    }

    fn empty_buckets() -> Vec<AgeBucket> {
        let mut from = 0;
        let mut buckets = Vec::with_capacity(AGE_EDGES_DAYS.len() + 1);
        for edge in AGE_EDGES_DAYS {
            buckets.push(AgeBucket {
                from_days: from,
                to_days: Some(edge),
                ..Default::default()
            });
            from = edge;
        }
        buckets.push(AgeBucket {
            from_days: from,
            to_days: None,
            ..Default::default()
        });
        buckets
    }

    fn age_matched(by_age: &[AgeBucket]) -> Option<AgeMatched> {
        let total_tagged: u64 = by_age.iter().map(|b| b.tagged.introduced).sum();
        let used: Vec<&AgeBucket> = by_age.iter().filter(|b| b.comparable()).collect();
        if used.is_empty() || total_tagged == 0 {
            return None;
        }
        let tagged_intro: u64 = used.iter().map(|b| b.tagged.introduced).sum();
        let tagged_surv: u64 = used.iter().map(|b| b.tagged.surviving).sum();
        let untagged_weighted: f64 = used
            .iter()
            .map(|b| b.tagged.introduced as f64 * b.untagged.survival_rate().unwrap_or(0.0))
            .sum();
        let tagged_rate = tagged_surv as f64 / tagged_intro as f64;
        let untagged_rate = untagged_weighted / tagged_intro as f64;
        Some(AgeMatched {
            tagged_rate,
            untagged_rate,
            gap: tagged_rate - untagged_rate,
            buckets_used: used.len(),
            tagged_lines_covered: tagged_intro as f64 / total_tagged as f64,
        })
    }
}

/// `YYYY-MM-DD` of a Unix timestamp, UTC.
pub fn ymd(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| ts.to_string())
}

/// The full audit result.
#[derive(Debug, Default, serde::Serialize)]
pub struct SurvivalReport {
    pub total_commits: u64,
    pub verified: SurvivalStat,
    pub probable: SurvivalStat,
    /// Per-agent verified stats.
    pub by_agent: BTreeMap<String, SurvivalStat>,
    pub baseline: Baseline,
    pub coverage: Coverage,
}

/// Pure aggregation: given per-commit introduced counts, detections, the
/// blame owner of every line at HEAD and the committer timestamp of HEAD,
/// compute the survival report.
pub fn compute_survival(
    commits: &[(CommitMeta, u64)],
    detections: &HashMap<String, Detection>,
    head_owners: &[String],
    head_committed_at: i64,
) -> SurvivalReport {
    let mut report = SurvivalReport {
        total_commits: commits.len() as u64,
        ..Default::default()
    };
    let mut by_age = Baseline::empty_buckets();

    // Surviving lines per commit hash.
    let mut surviving_by_hash: HashMap<&str, u64> = HashMap::new();
    for owner in head_owners {
        *surviving_by_hash.entry(owner.as_str()).or_default() += 1;
    }

    // Commits ordered by committer date: the first one owning a line at HEAD
    // is the horizon behind which nothing survives.
    let mut oldest_surviving: Option<(i64, u64)> = None;
    let mut before: Vec<(i64, bool, u64)> = Vec::new();

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
        // Committer clocks can run ahead of HEAD's; such a line is simply new.
        let age_days = if meta.committed_at > 0 {
            Some(((head_committed_at - meta.committed_at).max(0) / 86_400) as u64)
        } else {
            None
        };

        match class {
            EvidenceClass::Verified => {
                report.verified.record(*introduced, surviving);
                if let Some(d) = det {
                    report
                        .by_agent
                        .entry(d.agent.clone())
                        .or_default()
                        .record(*introduced, surviving);
                }
                if let Some(age) = age_days {
                    by_age[Baseline::bucket_index(age)]
                        .tagged
                        .record(*introduced, surviving);
                }
            }
            EvidenceClass::Probable => report.probable.record(*introduced, surviving),
            EvidenceClass::Unknown => {
                report.baseline.untagged.record(*introduced, surviving);
                if let Some(age) = age_days {
                    by_age[Baseline::bucket_index(age)]
                        .untagged
                        .record(*introduced, surviving);
                }
            }
        }

        if meta.committed_at > 0 {
            if surviving > 0 && oldest_surviving.is_none_or(|(ts, _)| meta.committed_at < ts) {
                oldest_surviving = Some((meta.committed_at, age_days.unwrap_or(0)));
            }
            before.push((
                meta.committed_at,
                class == EvidenceClass::Verified,
                *introduced,
            ));
        }
    }

    report.baseline.oldest_surviving = oldest_surviving.map(|(ts, age_days)| {
        let mut o = OldestSurviving {
            date: ymd(ts),
            age_days,
            ..Default::default()
        };
        for (committed_at, tagged, introduced) in &before {
            if *committed_at < ts {
                o.commits_before += 1;
                o.introduced_before += introduced;
                if *tagged {
                    o.tagged_commits_before += 1;
                    o.tagged_introduced_before += introduced;
                }
            }
        }
        o
    });
    report.baseline.age_matched = Baseline::age_matched(&by_age);
    report.baseline.by_age = by_age;

    report.coverage.small_sample = report.verified.commits < SAMPLE_FLOOR;
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
            "--notes=ai",
            "--pretty=format:%x00%H%x1f%ct%x1f%an%x1f%ae%x1f%B%x1f%N",
        ],
    )?;
    let mut commits = Vec::new();
    for record in raw.split('\u{0}') {
        if record.trim().is_empty() {
            continue;
        }
        let mut fields = record.splitn(6, '\u{1f}');
        let hash = fields.next().unwrap_or("").trim().to_string();
        let committed_at = fields
            .next()
            .unwrap_or("")
            .trim()
            .parse::<i64>()
            .unwrap_or(0);
        let author_name = fields.next().unwrap_or("").to_string();
        let author_email = fields.next().unwrap_or("").to_string();
        let message = fields.next().unwrap_or("").to_string();
        let notes = fields.next().unwrap_or("").trim().to_string();
        if hash.is_empty() {
            continue;
        }
        commits.push(CommitMeta {
            hash,
            author_name,
            author_email,
            message,
            notes,
            committed_at,
        });
    }
    Ok(commits)
}

/// Parse the output of `git log --numstat --format=%x00%H` into per-commit
/// added-line counts. One git traversal replaces one `git show` per commit.
pub fn parse_log_numstat(raw: &str) -> HashMap<String, u64> {
    let mut map = HashMap::new();
    for record in raw.split('\u{0}') {
        let record = record.trim_start_matches('\n');
        if record.trim().is_empty() {
            continue;
        }
        let (hash, rest) = record.split_once('\n').unwrap_or((record, ""));
        let hash = hash.trim();
        if hash.len() == 40 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
            map.insert(hash.to_string(), parse_numstat_added(rest));
        }
    }
    map
}

/// Added-line counts for every non-merge commit, in a single git call.
pub fn lines_added_all(dir: &Path) -> Result<HashMap<String, u64>> {
    let raw = git(dir, &["log", "--no-merges", "--numstat", "--format=%x00%H"])?;
    Ok(parse_log_numstat(&raw))
}

/// Blame flags of method v2. `-w` ignores whitespace so a re-indent or a
/// formatter pass does not re-attribute a line; `-M` follows lines moved
/// within a file; `-C` follows lines moved or copied from another file
/// touched by the same commit. Published in the report so a reader can
/// reproduce the exact blame.
pub const BLAME_FLAGS: [&str; 3] = ["-w", "-M", "-C"];

/// Conventional name of the revision list that `git blame` should skip
/// (mass reformats, renames-only commits). Honoured when present at the
/// repository root, as GitHub's blame view does.
pub const IGNORE_REVS_FILE: &str = ".git-blame-ignore-revs";

/// The repository's `.git-blame-ignore-revs`, when present and usable.
///
/// `git blame` dies on any entry it cannot resolve to a commit. Because a
/// per-file blame failure is tolerated (binary files), that would silently
/// turn every line of the repository into "no owner" and report 0 %
/// survival. A file with an unresolvable entry is therefore skipped with a
/// warning rather than passed through.
pub fn ignore_revs_file(dir: &Path) -> Option<std::path::PathBuf> {
    let path = dir.join(IGNORE_REVS_FILE);
    let text = std::fs::read_to_string(&path).ok()?;
    let revs: Vec<&str> = text
        .lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .collect();
    for rev in revs {
        let spec = format!("{rev}^{{commit}}");
        if git(
            dir,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                "--end-of-options",
                &spec,
            ],
        )
        .is_err()
        {
            eprintln!(
                "warning: {IGNORE_REVS_FILE} lists '{rev}', which is not a commit here; \
                 the file is ignored for this audit"
            );
            return None;
        }
    }
    Some(path)
}

/// Blame every tracked text file at HEAD, returning the owning commit of each
/// surviving line. `ignore_revs` is the resolved [`ignore_revs_file`].
pub fn blame_head(dir: &Path, ignore_revs: Option<&Path>) -> Result<Vec<String>> {
    let mut base_args: Vec<&str> = vec!["blame"];
    base_args.extend(BLAME_FLAGS);
    base_args.push("--line-porcelain");
    let ignore_flag = ignore_revs.map(|p| format!("--ignore-revs-file={}", p.display()));
    if let Some(flag) = ignore_flag.as_deref() {
        base_args.push(flag);
    }
    base_args.extend(["HEAD", "--"]);

    let files = git(dir, &["ls-files"])?;
    let list: Vec<&str> = files
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter(|l| !is_generated_path(l))
        .collect();
    let total = list.len();
    // Blame is embarrassingly parallel across files: each worker pulls the
    // next index from a shared atomic cursor and runs its own git subprocess.
    // Line order is irrelevant — owners are aggregated into per-commit counts.
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(2, 16);
    let cursor = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let owners: Vec<String> = std::thread::scope(|s| {
        let mut handles = Vec::new();
        for _ in 0..threads {
            handles.push(s.spawn(|| {
                let mut local: Vec<String> = Vec::new();
                loop {
                    let i = cursor.fetch_add(1, Ordering::Relaxed);
                    if i >= total {
                        break;
                    }
                    // Skip files git cannot blame (e.g. binary): tolerate
                    // per-file errors.
                    let mut args = base_args.clone();
                    args.push(list[i]);
                    if let Ok(porcelain) = git(dir, &args) {
                        local.extend(parse_blame_owners(&porcelain));
                    }
                    let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                    if total > 200 && d % 200 == 0 {
                        eprintln!("  blaming files at HEAD: {d}/{total}");
                    }
                }
                local
            }));
        }
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    });
    Ok(owners)
}

/// True when the repository is a shallow clone. Its history is truncated:
/// commits at the boundary appear to introduce every line of their tree and
/// blame cannot look past them, so every figure would be wrong.
pub fn is_shallow(dir: &Path) -> bool {
    match git(dir, &["rev-parse", "--is-shallow-repository"]) {
        Ok(out) => out.trim() == "true",
        // git before 2.15 lacks the query; the marker file is the fallback.
        Err(_) => git(dir, &["rev-parse", "--git-path", "shallow"])
            .map(|p| dir.join(p.trim()).exists())
            .unwrap_or_else(|_| dir.join(".git").join("shallow").exists()),
    }
}

/// The commit the working tree is at: what an audit measures and what an
/// audit seal is bound to.
pub fn head_commit(dir: &Path) -> Result<String> {
    let hash = git(dir, &["rev-parse", "--verify", "HEAD^{commit}"])?
        .trim()
        .to_string();
    if hash.len() != 40 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        anyhow::bail!("git rev-parse HEAD returned {hash:?}, not a commit hash");
    }
    Ok(hash)
}

/// Committer timestamp of HEAD: the moment the measured tree came to be,
/// from which every line's age is counted.
pub fn head_committed_at(dir: &Path) -> Result<i64> {
    let raw = git(dir, &["log", "-1", "--format=%ct", "HEAD"])?;
    raw.trim()
        .parse::<i64>()
        .with_context(|| format!("git log -1 --format=%ct returned {raw:?}"))
}

/// The `origin` remote URL, if the repository has one.
pub fn origin_url(dir: &Path) -> Option<String> {
    git(dir, &["remote", "get-url", "origin"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[derive(Debug, Default, Clone)]
pub struct AuditOptions {
    /// Measure a shallow clone anyway; the report then carries
    /// `coverage.shallow = true`.
    pub allow_shallow: bool,
}

/// The audit was refused because the clone is shallow. Typed so the CLI can
/// exit 2 (unusable request) rather than 1 (measurement failed).
#[derive(Debug, thiserror::Error)]
#[error(
    "refusing to audit a shallow clone: its history is truncated, so introduced and \
     surviving line counts would be wrong.\n  Fetch the full history first: \
     `git fetch --unshallow` (in GitHub Actions: `fetch-depth: 0` on actions/checkout), \
     or pass --allow-shallow to measure anyway and have the report say so."
)]
pub struct ShallowCloneRefused;

/// Full Group-0 audit of a git repository: no ledger, no hooks, no proxy.
pub fn audit_repo(dir: &Path, opts: &AuditOptions) -> Result<SurvivalReport> {
    let shallow = is_shallow(dir);
    if shallow && !opts.allow_shallow {
        return Err(ShallowCloneRefused.into());
    }
    if shallow {
        eprintln!(
            "warning: shallow clone; history is truncated and the figures below are partial \
             (coverage.shallow = true)"
        );
    }

    let commits = read_commits(dir)?;
    let detections: HashMap<String, Detection> = commits
        .iter()
        .filter_map(|c| detect_ai(c).map(|d| (c.hash.clone(), d)))
        .collect();

    // One git traversal counts the introduced lines of every commit: the
    // tagged ones for the headline, the untagged ones for the baseline.
    let added_by_hash = lines_added_all(dir)?;
    let with_intro: Vec<(CommitMeta, u64)> = commits
        .into_iter()
        .map(|c| {
            let introduced = added_by_hash.get(&c.hash).copied().unwrap_or(0);
            (c, introduced)
        })
        .collect();
    let head_committed_at = head_committed_at(dir)?;

    let ignore_revs = ignore_revs_file(dir);
    let head_owners = blame_head(dir, ignore_revs.as_deref())?;
    let mut report = compute_survival(&with_intro, &detections, &head_owners, head_committed_at);
    report.coverage.ignore_revs_file = ignore_revs.is_some();
    report.coverage.shallow = shallow;
    Ok(report)
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
            notes: String::new(),
            committed_at: 0,
        }
    }

    #[test]
    fn detects_git_ai_authorship_note_as_verified() {
        let mut c = meta(
            "1".repeat(40).as_str(),
            "Dev",
            "dev@example.com",
            "add parser",
        );
        c.notes = concat!(
            "src/lib.rs\ns_0123456789abcd::t_0123456789abcd 1-40\n---\n",
            "{\"schema_version\":\"authorship/3.0.0\",\"base_commit_sha\":\"x\",",
            "\"prompts\":{},\"sessions\":{\"s_0123456789abcd\":{\"agent_id\":",
            "{\"tool\":\"claude\",\"id\":\"a\",\"model\":\"claude-opus-4\"}}}}"
        )
        .into();
        let d = detect_ai(&c).expect("git-ai note should be detected");
        assert_eq!(d.agent, "claude-code");
        assert_eq!(classify(Some(&d)), EvidenceClass::Verified);
        assert!(d.evidence[0].contains("refs/notes/ai"));
    }

    fn noted(notes: &str) -> CommitMeta {
        let mut c = meta(
            "1".repeat(40).as_str(),
            "Dev",
            "dev@example.com",
            "add parser",
        );
        c.notes = notes.into();
        c
    }

    /// schema_version alone is a schema marker, not AI evidence.
    #[test]
    fn schema_only_git_ai_note_is_not_ai() {
        let c = noted("auth.py\n---\n{\"schema_version\":\"authorship/3.0.0\",\"prompts\":{}}");
        assert!(detect_ai(&c).is_none());
    }

    /// A human declaration in the note must not create generic AI attribution.
    #[test]
    fn human_only_git_ai_note_is_not_ai() {
        let c = noted(concat!(
            "auth.py\n  h1 1-2\n---\n",
            "{\"schema_version\":\"authorship/3.0.0\",\"prompts\":{},",
            "\"humans\":{\"h1\":{\"author\":\"Ada Human <ada@example.test>\"}}}"
        ));
        assert!(detect_ai(&c).is_none());
    }

    /// A non-empty tool is qualifying metadata, named as that tool.
    #[test]
    fn named_tool_git_ai_note_is_ai_metadata() {
        let c = noted(concat!(
            "auth.py\n  p_mock 20-30\n---\n",
            "{\"schema_version\":\"authorship/3.0.0\",",
            "\"prompts\":{\"p_mock\":{\"agent_id\":{\"tool\":\"mock_ai\"}}}}"
        ));
        let d = detect_ai(&c).expect("named tool");
        assert_eq!(d.agent, "mock_ai");
        assert_eq!(d.confidence, 1.0);
        assert_eq!(classify(Some(&d)), EvidenceClass::Verified);
        assert!(d.evidence[0].contains("refs/notes/ai"));
        assert!(!d.evidence[0].to_lowercase().contains("sign"));
    }

    /// An empty or whitespace tool is ignored. A later non-empty tool still counts.
    #[test]
    fn empty_or_blank_tool_is_not_ai_and_does_not_hide_a_later_tool() {
        for notes in [
            "auth.py\n---\n{\"schema_version\":\"authorship/3.0.0\",\"prompts\":{\"p\":{\"agent_id\":{\"tool\":\"\"}}}}",
            "auth.py\n---\n{\"schema_version\":\"authorship/3.0.0\",\"prompts\":{\"p\":{\"agent_id\":{\"tool\":\"   \"}}}}",
        ] {
            assert!(detect_ai(&noted(notes)).is_none(), "{notes}");
        }
        let c = noted(concat!(
            "auth.py\n---\n{\"schema_version\":\"authorship/3.0.0\",",
            "\"sessions\":{\"s\":{\"agent_id\":{\"tool\":\"\"}}},",
            "\"prompts\":{\"p\":{\"agent_id\":{\"tool\":\"mock_ai\"}}}}"
        ));
        assert_eq!(detect_ai(&c).map(|d| d.agent), Some("mock_ai".into()));
    }

    /// A note with no qualifying tool must not hide an independent trailer.
    #[test]
    fn schema_only_note_does_not_hide_an_independent_trailer() {
        let mut c = noted("auth.py\n---\n{\"schema_version\":\"authorship/3.0.0\",\"prompts\":{}}");
        c.message = "plain\n\nCo-Authored-By: Claude <noreply@anthropic.com>\n".into();
        let d = detect_ai(&c).expect("trailer");
        assert_eq!(d.agent, "claude-code");
        assert!(d.evidence[0].starts_with("trailer:"));
    }

    /// A note that is not JSON, or that names a tool without schema_version,
    /// supplies no git-ai signal.
    #[test]
    fn malformed_or_unschematized_git_ai_note_is_not_ai() {
        assert!(detect_ai(&noted("this note mentions schema_version but is not json")).is_none());
        assert!(
            detect_ai(&noted(
                "auth.py\n---\n{\"prompts\":{\"p\":{\"agent_id\":{\"tool\":\"mock_ai\"}}}}"
            ))
            .is_none()
        );
    }

    /// Two named tools: one deterministic agent, sessions before prompts,
    /// lexicographic key within a map. Not a proof of authorship.
    #[test]
    fn competing_git_ai_tools_follow_session_key_order() {
        let mock_on_a = noted(concat!(
            "{\"schema_version\":\"authorship/3.0.0\",\"sessions\":{",
            "\"z\":{\"agent_id\":{\"tool\":\"other_ai\"}},",
            "\"a\":{\"agent_id\":{\"tool\":\"mock_ai\"}}}}"
        ));
        assert_eq!(
            detect_ai(&mock_on_a).map(|d| d.agent),
            Some("mock_ai".into())
        );
        let other_on_a = noted(concat!(
            "{\"schema_version\":\"authorship/3.0.0\",\"sessions\":{",
            "\"z\":{\"agent_id\":{\"tool\":\"mock_ai\"}},",
            "\"a\":{\"agent_id\":{\"tool\":\"other_ai\"}}}}"
        ));
        assert_eq!(
            detect_ai(&other_on_a).map(|d| d.agent),
            Some("other_ai".into())
        );
        let sessions_first = noted(concat!(
            "{\"schema_version\":\"authorship/3.0.0\",",
            "\"prompts\":{\"p\":{\"agent_id\":{\"tool\":\"mock_ai\"}}},",
            "\"sessions\":{\"s\":{\"agent_id\":{\"tool\":\"other_ai\"}}}}"
        ));
        assert_eq!(
            detect_ai(&sessions_first).map(|d| d.agent),
            Some("other_ai".into())
        );
    }

    #[test]
    fn detects_ietf_and_ai_trailers_as_verified() {
        for (trailer, agent) in [
            ("Drafted-With: ~claude-opus-4", "claude-code"),
            ("Executed-By: ~devin-bot", "devin"),
            ("AI-Model: openai-codex/gpt-5", "openai-codex"),
            ("AI-Agent: SomeNewTool v2", "somenewtool"),
            ("AI-Session-ID: abc12", "ai"),
        ] {
            let c = meta(
                "2".repeat(40).as_str(),
                "Dev",
                "dev@example.com",
                &format!("fix\n\n{trailer}"),
            );
            let d = detect_ai(&c).unwrap_or_else(|| panic!("{trailer} not detected"));
            assert_eq!(d.agent, agent, "{trailer}");
            assert_eq!(classify(Some(&d)), EvidenceClass::Verified);
        }
    }

    #[test]
    fn trailer_block_follows_git_rules() {
        // Subject only: no trailer block.
        assert!(parse_trailers("Co-Authored-By: Claude <noreply@anthropic.com>").is_empty());
        // Last paragraph of trailers, with a folded continuation line.
        let t =
            parse_trailers("fix\n\nbody prose\n\nSigned-off-by: A <a@x>\nAI-Agent: Some\n  Tool\n");
        assert_eq!(t.len(), 2);
        assert_eq!(t[1].key, "ai-agent");
        assert_eq!(t[1].value, "Some Tool");
        // A prose line in the last paragraph disqualifies it...
        assert!(parse_trailers("fix\n\nExecuted-By: CI pipeline\nruns nightly.").is_empty());
        // ...unless git-generated trailers are present (≥ 25 % trailers).
        let t = parse_trailers("fix\n\n(cherry picked from commit abc)\nSigned-off-by: A <a@x>\n");
        assert_eq!(t.len(), 1);
        // A URL is prose, not a `https:` trailer.
        assert!(parse_trailers("fix\n\nhttps://example.com/issue/1").is_empty());
        // Trailers followed by another paragraph are body text.
        assert!(parse_trailers("fix\n\nAI-Agent: Cursor\n\nMore notes.").is_empty());
    }

    #[test]
    fn key_value_in_body_prose_is_not_a_trailer() {
        for message in [
            "deploy\n\nExecuted-By: CI pipeline after every merge.\nSee the runbook.",
            "deploy\n\nExecuted-By: CI pipeline\n\nRolled out to staging first.",
            "deploy\nAI-Agent: Cursor",
        ] {
            let c = meta("3".repeat(40).as_str(), "Dev", "dev@example.com", message);
            assert!(detect_ai(&c).is_none(), "misdetected: {message:?}");
        }
    }

    #[test]
    fn detects_assisted_by_trailer_as_verified() {
        for (value, agent) in [
            ("Claude Code (claude-sonnet-4)", "claude-code"),
            ("Claude:claude-3-5-sonnet-20241022", "claude-code"),
            ("Cursor", "cursor"),
            ("GitHub Copilot", "github-copilot"),
            ("Windsurf Cascade (gpt-5)", "windsurf-cascade"),
            ("Windsurf Cascade v2", "windsurf-cascade"),
        ] {
            let c = meta(
                "5".repeat(40).as_str(),
                "Dev",
                "dev@example.com",
                &format!("fix\n\nAssisted-by: {value}\nSigned-off-by: Dev <dev@example.com>"),
            );
            let d = detect_ai(&c).unwrap_or_else(|| panic!("{value} not detected"));
            assert_eq!(d.agent, agent, "{value}");
            assert_eq!(classify(Some(&d)), EvidenceClass::Verified);
            assert!(d.evidence[0].starts_with("trailer: Assisted-by:"));
        }
        let c = meta(
            "5".repeat(40).as_str(),
            "Dev",
            "dev@example.com",
            "fix\n\nAssisted-by: none",
        );
        assert!(detect_ai(&c).is_none());
    }

    #[test]
    fn detects_copilot_coding_agent_author_as_verified() {
        let c = meta(
            "6".repeat(40).as_str(),
            "copilot-swe-agent[bot]",
            "198982749+Copilot@users.noreply.github.com",
            "Fix flaky test\n\nCo-authored-by: Tarik <tarik@example.com>",
        );
        let d = detect_ai(&c).expect("should detect");
        assert_eq!(d.agent, "copilot");
        assert_eq!(classify(Some(&d)), EvidenceClass::Verified);
        assert!(d.evidence[0].starts_with("author:"));
    }

    #[test]
    fn coauthor_names_containing_agent_substrings_are_humans() {
        for trailer in [
            "Co-Authored-By: Raider Bot <raider@example.com>",
            "Co-Authored-By: Precursor Team <team@precursor.dev>",
            "Co-Authored-By: Claude Dupont <claude.dupont@example.fr>",
            "Co-Authored-By: Codexia Ltd <ops@codexia.example>",
        ] {
            let c = meta(
                "1".repeat(40).as_str(),
                "Tarik",
                "tarik@example.com",
                &format!("pair session\n\n{trailer}"),
            );
            assert!(detect_ai(&c).is_none(), "misdetected: {trailer}");
        }
        // Whole-word product names still match, whatever surrounds them.
        for (trailer, agent) in [
            ("Co-Authored-By: aider (gpt-4o) <aider@aider.chat>", "aider"),
            (
                "Co-Authored-By: Copilot <175728472+Copilot@users.noreply.github.com>",
                "github-copilot",
            ),
            (
                "Co-Authored-By: Claude Opus 4.1 <noreply@anthropic.com>",
                "claude-code",
            ),
        ] {
            let c = meta(
                "1".repeat(40).as_str(),
                "Tarik",
                "tarik@example.com",
                &format!("pair session\n\n{trailer}"),
            );
            assert_eq!(
                detect_ai(&c).map(|d| d.agent).as_deref(),
                Some(agent),
                "{trailer}"
            );
        }
        // A person whose address merely contains "aider" is not the tool.
        let c = meta(
            "1".repeat(40).as_str(),
            "Tarik",
            "raider@example.com",
            "manual fix",
        );
        assert!(detect_ai(&c).is_none());
    }

    #[test]
    fn plain_message_with_colon_is_not_ai() {
        let c = meta(
            "3".repeat(40).as_str(),
            "Dev",
            "dev@example.com",
            "fix: handle timeout\n\nNote: see issue #12",
        );
        assert!(detect_ai(&c).is_none());
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
    fn parse_log_numstat_splits_per_commit() {
        let a = "a".repeat(40);
        let b = "b".repeat(40);
        let raw = format!(
            "\u{0}{a}\n10\t2\tsrc/main.rs\n5\t0\tREADME.md\n\u{0}{b}\n7\t1\tsrc/lib.rs\n3\t0\tpackage-lock.json\n"
        );
        let map = parse_log_numstat(&raw);
        assert_eq!(map.get(a.as_str()), Some(&15));
        // Lockfile excluded: only src/lib.rs counts.
        assert_eq!(map.get(b.as_str()), Some(&7));
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

    #[test]
    fn coauthor_humans_named_like_agents_are_not_misdetected() {
        // Devin, Jules and Gemini are people's names; "precursor" contains
        // "cursor". Without a vendor/bot address these are humans.
        for trailer in [
            "Co-Authored-By: Devin Jones <devin@example.com>",
            "Co-Authored-By: Jules Verne <jules.verne@nautilus.fr>",
            "Co-Authored-By: Gemini Rivera <gemini@example.org>",
            "Co-Authored-By: Precursor Team <team@precursor.dev>",
        ] {
            let c = meta(
                "1".repeat(40).as_str(),
                "Tarik",
                "tarik@example.com",
                &format!("pair session\n\n{trailer}"),
            );
            assert!(detect_ai(&c).is_none(), "misdetected: {trailer}");
        }
    }

    #[test]
    fn coauthor_bot_identities_still_detected() {
        for (trailer, agent) in [
            (
                "Co-Authored-By: google-labs-jules[bot] <161369871+google-labs-jules[bot]@users.noreply.github.com>",
                "jules",
            ),
            (
                "Co-Authored-By: Cursor Agent <cursoragent@cursor.com>",
                "cursor",
            ),
            (
                "Co-Authored-By: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>",
                "gemini",
            ),
        ] {
            let c = meta(
                "2".repeat(40).as_str(),
                "Dev",
                "dev@example.com",
                &format!("change\n\n{trailer}"),
            );
            let d = detect_ai(&c).unwrap_or_else(|| panic!("should detect: {trailer}"));
            assert_eq!(d.agent, agent);
        }
    }

    #[test]
    fn negative_disclosure_trailers_are_not_ai() {
        // Projects with a disclosure policy stamp every human commit.
        for trailer in [
            "AI-Assisted: no",
            "AI-Generated: false",
            "Drafted-With: none",
            "Executed-By: human",
            "AI-Model: N/A",
        ] {
            let c = meta(
                "4".repeat(40).as_str(),
                "Dev",
                "dev@example.com",
                &format!("fix typo\n\n{trailer}"),
            );
            assert!(detect_ai(&c).is_none(), "misdetected: {trailer}");
        }
        // ...while positive values still count.
        let c = meta(
            "4".repeat(40).as_str(),
            "Dev",
            "dev@example.com",
            "fix typo\n\nAI-Assisted: yes",
        );
        assert_eq!(detect_ai(&c).map(|d| d.agent), Some("ai".to_string()));
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

        let report = compute_survival(&commits, &detections, &head_owners, 0);
        assert_eq!(report.total_commits, 2);
        assert_eq!(report.verified.commits, 1);
        assert_eq!(report.verified.introduced, 5);
        assert_eq!(report.verified.surviving, 3);
        assert_eq!(report.verified.survival_rate(), Some(0.6));
        // Human commit contributes nothing to any attributed class; it is
        // the baseline.
        assert_eq!(report.probable, SurvivalStat::default());
        assert_eq!(report.baseline.untagged.commits, 1);
        assert_eq!(report.baseline.untagged.introduced, 100);
        assert_eq!(report.baseline.untagged.surviving, 50);
        // Per-agent aggregation present.
        assert_eq!(report.by_agent["claude-code"].surviving, 3);
    }

    const DAY: i64 = 86_400;

    fn dated(hash: &str, message: &str, committed_at: i64) -> CommitMeta {
        let mut c = meta(&hash.repeat(40), "x", "x@x", message);
        c.committed_at = committed_at;
        c
    }

    fn owners(pairs: &[(&CommitMeta, u64)]) -> Vec<String> {
        pairs
            .iter()
            .flat_map(|(c, n)| std::iter::repeat_n(c.hash.clone(), *n as usize))
            .collect()
    }

    #[test]
    fn baseline_puts_untagged_lines_next_to_tagged_ones_of_the_same_age() {
        let head = 1_000 * DAY;
        // Two windows, 0-30 d and 90-180 d, each with five tagged and five
        // untagged commits so both are comparable; one lone tagged commit at
        // 400 d with no untagged counterpart.
        // (age in days, tagged, introduced, surviving)
        let mut plan: Vec<(i64, bool, u64, u64)> = Vec::new();
        for _ in 0..5 {
            plan.push((10, true, 100, 90)); // tagged, young: 90 %
            plan.push((10, false, 100, 80)); // untagged, young: 80 %
            plan.push((120, true, 100, 40)); // tagged, older: 40 %
            plan.push((120, false, 100, 60)); // untagged, older: 60 %
        }
        plan.push((400, true, 1_000, 0));
        let mut commits = Vec::new();
        let mut detections = HashMap::new();
        let mut alive: Vec<(CommitMeta, u64)> = Vec::new();
        for (i, (age, tagged, intro, surv)) in plan.into_iter().enumerate() {
            let mut c = dated("0", if tagged { "ai" } else { "hand" }, head - age * DAY);
            c.hash = format!("{i:040x}");
            if tagged {
                detections.insert(c.hash.clone(), detection("claude-code", 1.0));
            }
            commits.push((c.clone(), intro));
            alive.push((c, surv));
        }
        let owner_pairs: Vec<(&CommitMeta, u64)> = alive.iter().map(|(c, s)| (c, *s)).collect();
        let report = compute_survival(&commits, &detections, &owners(&owner_pairs), head);

        let by_age = &report.baseline.by_age;
        assert_eq!(by_age.len(), AGE_EDGES_DAYS.len() + 1);
        assert_eq!((by_age[0].from_days, by_age[0].to_days), (0, Some(30)));
        assert_eq!(by_age[5].to_days, None);
        assert_eq!(by_age[0].tagged.commits, 5);
        assert_eq!(by_age[0].untagged.commits, 5);
        assert!(approx(by_age[0].tagged.survival_rate(), 0.9));
        assert!(approx(by_age[0].untagged.survival_rate(), 0.8));
        assert!(approx(by_age[2].tagged.survival_rate(), 0.4));
        assert!(approx(by_age[2].untagged.survival_rate(), 0.6));
        assert!(by_age[0].comparable() && by_age[2].comparable());
        // The lone 400 d commit sits in 365-730 d without a counterpart.
        assert_eq!(by_age[4].tagged.commits, 1);
        assert!(!by_age[4].comparable());

        // Age-matched: tagged 650/1000 = 0.65; untagged re-weighted by the
        // tagged line mix (500 young at 0.8, 500 older at 0.6) = 0.70.
        let m = report
            .baseline
            .age_matched
            .as_ref()
            .expect("two comparable buckets");
        assert_eq!(m.buckets_used, 2);
        assert!(approx(Some(m.tagged_rate), 0.65));
        assert!(approx(Some(m.untagged_rate), 0.70));
        assert!(approx(Some(m.gap), -0.05));
        // 1,000 of the 2,000 tagged lines had no counterpart of their age.
        assert!(approx(Some(m.tagged_lines_covered), 0.5));

        // Untagged totals and the oldest surviving line (a 120 d commit; the
        // 400 d one owns nothing at HEAD and is the only commit before it).
        assert_eq!(report.baseline.untagged.commits, 10);
        let o = report.baseline.oldest_surviving.as_ref().unwrap();
        assert_eq!(o.age_days, 120);
        assert_eq!(o.date, ymd(head - 120 * DAY));
        assert_eq!(o.commits_before, 1);
        assert_eq!(o.tagged_commits_before, 1);
        assert_eq!(o.tagged_introduced_before, 1_000);
    }

    #[test]
    fn age_matched_needs_a_counterpart_in_at_least_one_window() {
        let head = 1_000 * DAY;
        let ai = dated("a", "ai", head - 10 * DAY);
        let hand = dated("b", "hand", head - 500 * DAY);
        let mut detections = HashMap::new();
        detections.insert(ai.hash.clone(), detection("claude-code", 1.0));
        let commits = vec![(ai.clone(), 10u64), (hand.clone(), 10u64)];
        let report = compute_survival(
            &commits,
            &detections,
            &owners(&[(&ai, 5), (&hand, 5)]),
            head,
        );
        assert!(report.baseline.age_matched.is_none());
        // A commit dated ahead of HEAD is simply new, not negative.
        let future = dated("c", "hand", head + 3 * DAY);
        let report = compute_survival(
            &[(future.clone(), 4)],
            &HashMap::new(),
            &owners(&[(&future, 4)]),
            head,
        );
        assert_eq!(report.baseline.by_age[0].untagged.commits, 1);
        // A missing committer date keeps the commit out of the by-age table
        // but not out of the totals.
        let undated = dated("d", "hand", 0);
        let report = compute_survival(
            &[(undated.clone(), 4)],
            &HashMap::new(),
            &owners(&[(&undated, 4)]),
            head,
        );
        assert_eq!(report.baseline.untagged.commits, 1);
        assert!(
            report
                .baseline
                .by_age
                .iter()
                .all(|b| b.untagged.commits == 0)
        );
        assert!(report.baseline.oldest_surviving.is_none());
    }

    #[test]
    fn a_cleared_repository_shows_as_commits_before_the_oldest_surviving_line() {
        // Old history, tagged and untagged, owns nothing at HEAD: the
        // repository was emptied and rebuilt 100 days ago.
        let head = 2_000 * DAY;
        let mut commits = Vec::new();
        let mut detections = HashMap::new();
        let mut live = Vec::new();
        for i in 0..8i64 {
            let mut c = dated(
                "0",
                if i % 2 == 0 { "ai" } else { "hand" },
                head - (600 + i) * DAY,
            );
            c.hash = format!("{i:040x}");
            if i % 2 == 0 {
                detections.insert(c.hash.clone(), detection("openhands", 1.0));
            }
            commits.push((c, 50));
        }
        let mut rebuilt = dated("0", "hand", head - 100 * DAY);
        rebuilt.hash = format!("{:040x}", 77);
        commits.push((rebuilt.clone(), 500));
        live.push((&rebuilt, 500));
        let report = compute_survival(&commits, &detections, &owners(&live), head);

        let o = report.baseline.oldest_surviving.as_ref().unwrap();
        assert_eq!(o.age_days, 100);
        assert_eq!(o.commits_before, 8);
        assert_eq!(o.introduced_before, 400);
        assert_eq!(o.tagged_commits_before, 4);
        assert_eq!(o.tagged_introduced_before, 200);
        // The headline still says 0 %: the number is right, the baseline
        // says what it measures.
        assert_eq!(report.verified.survival_rate(), Some(0.0));
        assert_eq!(
            report.baseline.by_age[4].untagged.survival_rate(),
            Some(0.0)
        );
        let v = serde_json::to_value(&report).unwrap();
        assert_eq!(v["baseline"]["oldest_surviving"]["commits_before"], 8);
        assert_eq!(v["baseline"]["by_age"].as_array().unwrap().len(), 6);
        assert!(v["baseline"]["by_age"][0].get("tagged").is_some());
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

        let report = compute_survival(&commits, &detections, &owners, 0);
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

        let report = compute_survival(&commits, &detections, &owners, 0);
        assert_eq!(report.verified.surviving, 2);
        assert_eq!(report.verified.survival_rate(), Some(1.0));
    }

    #[test]
    fn survival_rate_is_none_when_nothing_introduced() {
        let stat = SurvivalStat::default();
        assert_eq!(stat.survival_rate(), None);
        assert_eq!(stat.median_survival(), None);
        assert_eq!(stat.capped_survival_rate(), None);
        assert_eq!(stat.cap_lines(), None);
        assert_eq!(stat.largest_commit_share(), None);
        // A commit with no text lines has no rate and does not enter the
        // median or the cap.
        let mut stat = SurvivalStat::default();
        stat.record(0, 0);
        assert_eq!(stat.commits, 1);
        assert_eq!(stat.median_survival(), None);
    }

    fn approx(a: Option<f64>, b: f64) -> bool {
        a.is_some_and(|a| (a - b).abs() < 1e-9)
    }

    #[test]
    fn median_and_capped_rate_resist_one_bulk_commit() {
        // 19 ordinary commits at 90 %, one 5,000-line drop at 1 %.
        let mut stat = SurvivalStat::default();
        for _ in 0..19 {
            stat.record(100, 90);
        }
        stat.record(5000, 50);

        assert!(approx(stat.survival_rate(), 1760.0 / 6900.0));
        assert!(approx(stat.median_survival(), 0.9));
        // p95 by nearest rank over 20 sizes is the 19th: 100 lines.
        assert_eq!(stat.cap_lines(), Some(100));
        assert!(approx(stat.capped_survival_rate(), 1711.0 / 2000.0));
        assert!(approx(stat.largest_commit_share(), 5000.0 / 6900.0));
        assert!(stat.dominated_by_one_commit());
    }

    #[test]
    fn cap_falls_back_to_the_absolute_ceiling_on_small_samples() {
        // With fewer than 20 commits the 95th percentile is the largest
        // commit itself; only the 10,000-line ceiling limits its weight.
        let mut stat = SurvivalStat::default();
        for _ in 0..4 {
            stat.record(100, 90);
        }
        stat.record(100_000, 1000);
        assert_eq!(stat.cap_lines(), Some(CAP_CEILING_LINES));
        assert!(approx(stat.capped_survival_rate(), 460.0 / 10_400.0));
        // A group with only small commits: the cap is its largest commit and
        // the capped rate equals the line-weighted one.
        let mut small = SurvivalStat::default();
        small.record(10, 5);
        small.record(30, 30);
        assert_eq!(small.cap_lines(), Some(30));
        assert!(approx(
            small.capped_survival_rate(),
            small.survival_rate().unwrap()
        ));
        assert!(approx(small.median_survival(), 0.75));
        assert!(small.dominated_by_one_commit());
    }

    #[test]
    fn serialized_stat_carries_derived_fields() {
        let mut stat = SurvivalStat::default();
        stat.record(10, 5);
        let v = serde_json::to_value(&stat).unwrap();
        for key in [
            "commits",
            "introduced",
            "surviving",
            "survival_rate",
            "median_survival",
            "capped_survival_rate",
            "cap_lines",
            "largest_commit_share",
        ] {
            assert!(v.get(key).is_some(), "missing {key}");
        }
        assert_eq!(v["survival_rate"], 0.5);
        assert!(v.get("per_commit").is_none());
    }

    #[test]
    fn coverage_states_method_flags_and_sample_size() {
        let c = meta(&"a".repeat(40), "x", "x@x", "m");
        let mut detections = HashMap::new();
        detections.insert(c.hash.clone(), detection("claude-code", 1.0));
        let owners = vec![c.hash.clone()];
        let report = compute_survival(&[(c, 3)], &detections, &owners, 0);
        assert_eq!(report.coverage.method, "v4");
        assert_eq!(report.coverage.blame_flags, vec!["-w", "-M", "-C"]);
        assert_eq!(report.coverage.sample_floor, 5);
        assert!(report.coverage.small_sample);
        assert!(!report.coverage.shallow);

        let v = serde_json::to_value(&report).unwrap();
        assert_eq!(v["coverage"]["method"], "v4");
        assert_eq!(v["verified"]["survival_rate"], 1.0 / 3.0);
        assert_eq!(v["by_agent"]["claude-code"]["median_survival"], 1.0 / 3.0);
    }

    // -- end-to-end on a real synthetic git repo -------------------------------

    /// Git with a fixed identity and no commit signing: the user's global
    /// signing setup (a slow or interactive signer) must not shape the
    /// synthetic repositories these tests build.
    fn run_git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "tag.gpgsign=false",
                "-c",
                "trace2.eventTarget=",
            ])
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

    /// Empty repository on `main` with a human baseline commit.
    fn synthetic_repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        run_git(tmp.path(), &["init", "-q", "-b", "main"]);
        std::fs::write(tmp.path().join("main.py"), "print('hello')\n").unwrap();
        commit_all(tmp.path(), "initial scaffold");
        tmp
    }

    fn commit_all(dir: &Path, message: &str) {
        run_git(dir, &["add", "-A", "."]);
        run_git(dir, &["commit", "-q", "-m", message]);
    }

    const CLAUDE_TRAILER: &str = "\n\nCo-Authored-By: Claude <noreply@anthropic.com>";

    fn head_hash(dir: &Path) -> String {
        git(dir, &["rev-parse", "HEAD"]).unwrap().trim().to_string()
    }

    #[test]
    fn audits_a_synthetic_repo_end_to_end() {
        let tmp = synthetic_repo();
        let dir = tmp.path();

        // Commit 2 (AI, Claude trailer): adds 3 lines.
        std::fs::write(
            dir.join("auth.py"),
            "def refresh(user):\n    token = rotate(user)\n    return token\n",
        )
        .unwrap();
        commit_all(dir, &format!("add token refresh{CLAUDE_TRAILER}"));

        // Commit 3 (human): deletes one AI line -> 2 of 3 AI lines survive.
        std::fs::write(
            dir.join("auth.py"),
            "def refresh(user):\n    return rotate(user)\n",
        )
        .unwrap();
        commit_all(dir, "simplify refresh by hand");

        let report = audit_repo(dir, &AuditOptions::default()).expect("audit must succeed");

        assert_eq!(report.total_commits, 3);
        assert_eq!(report.verified.commits, 1);
        assert_eq!(report.verified.introduced, 3);
        // "def refresh(user):" survives; "return token"/"token = rotate" were
        // replaced. Exactly 1 original AI line remains attributable at HEAD.
        assert_eq!(report.verified.surviving, 1);
        assert!(report.by_agent.contains_key("claude-code"));

        // The two hand-written commits are the baseline: the scaffold line
        // and the one rewritten line of the refresh body both stand at HEAD.
        let b = &report.baseline;
        assert_eq!(b.untagged.commits, 2);
        assert_eq!(b.untagged.introduced, 2);
        assert_eq!(b.untagged.surviving, 2);
        // Everything was committed moments ago: one age window holds it all,
        // and the oldest surviving line is the scaffold with nothing before it.
        assert_eq!(b.by_age[0].tagged.commits, 1);
        assert_eq!(b.by_age[0].untagged.commits, 2);
        let o = b.oldest_surviving.as_ref().expect("scaffold line survives");
        assert_eq!(o.commits_before, 0);
        assert_eq!(o.age_days, 0);
        assert_eq!(o.date, ymd(head_committed_at(dir).unwrap()));
        // Below the sample floor on both sides: no age-matched figure.
        assert!(b.age_matched.is_none());
    }

    #[test]
    fn reindented_ai_line_still_survives() {
        let tmp = synthetic_repo();
        let dir = tmp.path();

        std::fs::write(
            dir.join("auth.py"),
            "def refresh(user):\n    token = rotate(user)\n    return token\n",
        )
        .unwrap();
        commit_all(dir, &format!("add token refresh{CLAUDE_TRAILER}"));

        // A human wraps the body in a block: every AI line is re-indented,
        // none is rewritten. Whitespace-insensitive blame keeps all three.
        std::fs::write(
            dir.join("auth.py"),
            "def refresh(user):\n        token = rotate(user)\n        return token\n",
        )
        .unwrap();
        commit_all(dir, "re-indent by hand");

        let report = audit_repo(dir, &AuditOptions::default()).expect("audit must succeed");
        assert_eq!(report.verified.introduced, 3);
        assert_eq!(report.verified.surviving, 3);
    }

    #[test]
    fn ai_block_moved_to_another_file_still_survives() {
        let tmp = synthetic_repo();
        let dir = tmp.path();

        // Long enough for blame's -C heuristic (≥ 40 alphanumerics moved).
        let block = "def rotate_credentials(user, issuer):\n    \
                     material = issuer.derive_material(user.identifier)\n    \
                     user.credentials = Credentials.from_material(material)\n    \
                     return user.credentials\n";
        std::fs::write(dir.join("auth.py"), block).unwrap();
        commit_all(dir, &format!("add credential rotation{CLAUDE_TRAILER}"));

        // Human moves the function to a new module in one commit.
        std::fs::remove_file(dir.join("auth.py")).unwrap();
        std::fs::write(
            dir.join("credentials.py"),
            format!("import issuers\n\n{block}"),
        )
        .unwrap();
        commit_all(dir, "move rotation into credentials module");

        let report = audit_repo(dir, &AuditOptions::default()).expect("audit must succeed");
        assert_eq!(report.verified.introduced, 4);
        assert_eq!(report.verified.surviving, 4);
    }

    #[test]
    fn blame_ignore_revs_file_is_honoured() {
        let tmp = synthetic_repo();
        let dir = tmp.path();

        std::fs::write(dir.join("config.py"), "NAME = 'causari'\nRETRIES = 3\n").unwrap();
        commit_all(dir, &format!("add config{CLAUDE_TRAILER}"));

        // Quote-style reformat: not whitespace, so only the ignore list can
        // keep the attribution on the AI commit.
        std::fs::write(dir.join("config.py"), "NAME = \"causari\"\nRETRIES = 3\n").unwrap();
        commit_all(dir, "reformat quotes");
        let reformat = head_hash(dir);

        let without = audit_repo(dir, &AuditOptions::default()).unwrap();
        assert_eq!(without.verified.surviving, 1);

        std::fs::write(
            dir.join(IGNORE_REVS_FILE),
            format!("# formatter passes\n{reformat}\n"),
        )
        .unwrap();
        assert!(ignore_revs_file(dir).is_some());
        let with = audit_repo(dir, &AuditOptions::default()).unwrap();
        assert_eq!(with.verified.introduced, 2);
        assert_eq!(with.verified.surviving, 2);
        assert!(with.coverage.ignore_revs_file);
        assert!(!without.coverage.ignore_revs_file);
    }

    #[test]
    fn unresolvable_ignore_revs_entry_disables_the_file() {
        let tmp = synthetic_repo();
        let dir = tmp.path();
        std::fs::write(
            dir.join(IGNORE_REVS_FILE),
            format!("{}\n{}\n", head_hash(dir), "0".repeat(40)),
        )
        .unwrap();
        assert!(ignore_revs_file(dir).is_none());
        // The audit still runs and still attributes lines (the ignore file
        // itself is part of this commit's introduced lines).
        std::fs::write(dir.join("x.py"), "x = 1\n").unwrap();
        commit_all(dir, &format!("add x{CLAUDE_TRAILER}"));
        let report = audit_repo(dir, &AuditOptions::default()).unwrap();
        assert!(report.verified.introduced > 0);
        assert_eq!(report.verified.surviving, report.verified.introduced);
    }

    /// Expected: a person whose author name is Claude, Devin, Jules, Gemini
    /// or Cursor, and whose email is their own, is UNKNOWN. The bot address
    /// still matches.
    /// `Verified` on that address is metadata, not a signature.
    #[test]
    fn human_authors_named_like_agents_are_unknown() {
        for (name, email) in [
            ("Claude", "claude.person@example.com"),
            ("Devin", "devin@example.com"),
            ("Jules", "jules@example.com"),
            ("Gemini", "gemini.person@example.com"),
            ("Cursor", "cursor.person@example.com"),
        ] {
            let c = meta("a".repeat(40).as_str(), name, email, "hand written");
            assert!(detect_ai(&c).is_none(), "{name} <{email}>");
            assert_eq!(classify(None), EvidenceClass::Unknown);
        }
        let bot = meta(
            "b".repeat(40).as_str(),
            "Claude",
            "noreply@anthropic.com",
            "hand written",
        );
        let d = detect_ai(&bot).expect("bot address");
        assert_eq!(d.agent, "claude-code");
        assert_eq!(classify(Some(&d)), EvidenceClass::Verified);
        assert!(d.evidence[0].starts_with("author:"));
        assert!(!d.evidence[0].to_lowercase().contains("sign"));
    }

    /// Expected: a trailer, a forged git-ai note and a spoofed bot email are
    /// `Verified` because the metadata matched a rule. The evidence string
    /// names the metadata. It does not say the commit was signed or that a
    /// model typed the code. A note that is not the authorship log, and a
    /// trailer key written as prose, match nothing.
    #[test]
    fn metadata_matches_are_not_signatures() {
        let forged = {
            let mut c = meta(
                "c".repeat(40).as_str(),
                "Dev",
                "dev@example.com",
                "add parser",
            );
            c.notes = concat!(
                "src/lib.rs\n---\n",
                "{\"schema_version\":\"authorship/3.0.0\",",
                "\"sessions\":{\"s\":{\"agent_id\":{\"tool\":\"claude\"}}}}"
            )
            .into();
            c
        };
        let d = detect_ai(&forged).expect("forged note still matches the rule");
        assert_eq!(classify(Some(&d)), EvidenceClass::Verified);
        assert!(d.evidence[0].contains("refs/notes/ai"));
        assert!(!d.evidence[0].to_lowercase().contains("sign"));

        let spoof = meta(
            "d".repeat(40).as_str(),
            "Mallory",
            "copilot-swe-agent@evil.example",
            "fix",
        );
        let d = detect_ai(&spoof).expect("the email string matches");
        assert_eq!(d.agent, "copilot");
        assert_eq!(classify(Some(&d)), EvidenceClass::Verified);
        assert!(d.evidence[0].starts_with("author:"));

        let mut garbage = meta(
            "e".repeat(40).as_str(),
            "Dev",
            "dev@example.com",
            "add parser",
        );
        garbage.notes = "this is not an authorship log".into();
        assert!(detect_ai(&garbage).is_none());

        for message in [
            "Claude wrote this function by hand",
            "fix\n\nAI-Agent:\n",
            "fix\n\nAI-Agent: \n",
        ] {
            let c = meta("f".repeat(40).as_str(), "Dev", "dev@example.com", message);
            assert!(detect_ai(&c).is_none(), "{message:?}");
        }
    }

    /// Expected: a fake `Co-Authored-By: Claude` on a human commit is
    /// `Verified`. That class means the trailer is there. The lines are
    /// counted with the tagged cohort. This is a false positive for
    /// "a model typed this", and the test locks it as metadata.
    #[test]
    fn fake_trailer_is_verified_metadata() {
        let tmp = synthetic_repo();
        let dir = tmp.path();
        std::fs::write(
            dir.join("auth.py"),
            "def refresh(user):\n    return token\n",
        )
        .unwrap();
        commit_all(dir, &format!("hand written{CLAUDE_TRAILER}"));
        let report = audit_repo(dir, &AuditOptions::default()).unwrap();
        assert_eq!(report.verified.commits, 1);
        assert_eq!(report.verified.introduced, 2);
        assert_eq!(report.verified.surviving, 2);
        assert!(report.by_agent.contains_key("claude-code"));
    }

    /// The commit object does not include refs/notes/ai. Adding and removing
    /// a note leaves the SHA unchanged and can change the class.
    #[test]
    fn note_mutation_does_not_change_commit_sha() {
        let tmp = synthetic_repo();
        let dir = tmp.path();
        std::fs::write(dir.join("auth.py"), "line\n").unwrap();
        commit_all(dir, "hand written");
        let sha = head_hash(dir);
        let before = audit_repo(dir, &AuditOptions::default()).unwrap();
        assert_eq!(before.verified.commits, 0);

        let note_path = dir.join("note.txt");
        std::fs::write(
            &note_path,
            concat!(
                "auth.py\n  p 1\n---\n",
                "{\"schema_version\":\"authorship/3.0.0\",",
                "\"prompts\":{\"p\":{\"agent_id\":{\"tool\":\"mock_ai\"}}}}\n"
            ),
        )
        .unwrap();
        run_git(
            dir,
            &[
                "notes",
                "--ref=ai",
                "add",
                "-f",
                "-F",
                note_path.to_str().unwrap(),
                "HEAD",
            ],
        );
        assert_eq!(head_hash(dir), sha);
        let mid = audit_repo(dir, &AuditOptions::default()).unwrap();
        assert_eq!(mid.verified.commits, 1);
        assert!(mid.by_agent.contains_key("mock_ai"));

        run_git(dir, &["notes", "--ref=ai", "remove", "HEAD"]);
        assert_eq!(head_hash(dir), sha);
        let after = audit_repo(dir, &AuditOptions::default()).unwrap();
        assert_eq!(after.verified.commits, 0);
    }

    /// A named tool on a line range classifies the commit. Survival then
    /// counts that commit's introduced lines, not the range. Method
    /// granularity, not line attribution.
    #[test]
    fn named_tool_range_classifies_the_whole_commit() {
        let tmp = synthetic_repo();
        let dir = tmp.path();
        let body: String = (1..=100).map(|i| format!("line {i}\n")).collect();
        std::fs::write(dir.join("auth.py"), body).unwrap();
        commit_all(dir, "hundred lines");
        let note_path = dir.join("note.txt");
        std::fs::write(
            &note_path,
            concat!(
                "auth.py\n  p_mock 20-30\n---\n",
                "{\"schema_version\":\"authorship/3.0.0\",",
                "\"prompts\":{\"p_mock\":{\"agent_id\":{\"tool\":\"mock_ai\"}}}}\n"
            ),
        )
        .unwrap();
        run_git(
            dir,
            &[
                "notes",
                "--ref=ai",
                "add",
                "-f",
                "-F",
                note_path.to_str().unwrap(),
                "HEAD",
            ],
        );
        let report = audit_repo(dir, &AuditOptions::default()).unwrap();
        assert_eq!(report.verified.commits, 1);
        assert_eq!(report.verified.introduced, 100);
        assert!(report.by_agent.contains_key("mock_ai"));
        assert!(!report.by_agent.contains_key("ai"));
    }

    /// Expected: cherry-pick, squash and amend that drop the trailer leave
    /// the lines untagged. The original AI commit is no longer what blame
    /// can see. UNKNOWN, not recovered.
    #[test]
    fn history_rewrite_that_drops_the_trailer_is_unknown() {
        // Cherry-pick without the message.
        let tmp = synthetic_repo();
        let dir = tmp.path();
        run_git(dir, &["checkout", "-q", "-b", "side"]);
        std::fs::write(
            dir.join("auth.py"),
            "def refresh(user):\n    return token\n",
        )
        .unwrap();
        commit_all(dir, &format!("add token refresh{CLAUDE_TRAILER}"));
        run_git(dir, &["checkout", "-q", "main"]);
        run_git(dir, &["cherry-pick", "-n", "side"]);
        commit_all(dir, "bring refresh across by hand");
        let report = audit_repo(dir, &AuditOptions::default()).unwrap();
        assert_eq!(report.verified.commits, 0);
        assert_eq!(report.verified.surviving, 0);

        // Squash of a tagged commit and a follow-up, message without a trailer.
        let tmp = synthetic_repo();
        let dir = tmp.path();
        std::fs::write(
            dir.join("auth.py"),
            "def refresh(user):\n    return token\n",
        )
        .unwrap();
        commit_all(dir, &format!("add token refresh{CLAUDE_TRAILER}"));
        std::fs::write(
            dir.join("auth.py"),
            "def refresh(user):\n    return token\n# note\n",
        )
        .unwrap();
        commit_all(dir, "human follow-up");
        run_git(dir, &["reset", "-q", "--soft", "HEAD~2"]);
        commit_all(dir, "squash without a trailer");
        let report = audit_repo(dir, &AuditOptions::default()).unwrap();
        assert_eq!(report.verified.commits, 0);

        // Amend strips the trailer and keeps the patch.
        let tmp = synthetic_repo();
        let dir = tmp.path();
        std::fs::write(
            dir.join("auth.py"),
            "def refresh(user):\n    return token\n",
        )
        .unwrap();
        commit_all(dir, &format!("add token refresh{CLAUDE_TRAILER}"));
        run_git(
            dir,
            &[
                "commit",
                "-q",
                "--amend",
                "-m",
                "same patch, trailer removed",
            ],
        );
        let report = audit_repo(dir, &AuditOptions::default()).unwrap();
        assert_eq!(report.verified.commits, 0);
        assert!(report.baseline.untagged.introduced >= 2);
    }

    /// Expected: a merge commit does not invent a tag and does not erase
    /// the tagged parent. The AI lines still survive. The merge commit
    /// itself is untagged.
    #[test]
    fn merge_commit_keeps_the_tagged_parent() {
        let tmp = synthetic_repo();
        let dir = tmp.path();
        run_git(dir, &["checkout", "-q", "-b", "feature"]);
        std::fs::write(
            dir.join("auth.py"),
            "def refresh(user):\n    return token\n",
        )
        .unwrap();
        commit_all(dir, &format!("add token refresh{CLAUDE_TRAILER}"));
        run_git(dir, &["checkout", "-q", "main"]);
        run_git(
            dir,
            &["merge", "-q", "--no-ff", "-m", "merge feature", "feature"],
        );
        let report = audit_repo(dir, &AuditOptions::default()).unwrap();
        assert_eq!(report.verified.commits, 1);
        assert_eq!(report.verified.introduced, 2);
        assert_eq!(report.verified.surviving, 2);
    }

    /// Expected: a shallow clone is refused. With `--allow-shallow` the
    /// report says `coverage.shallow` and does not pretend the history
    /// is complete.
    #[test]
    fn shallow_clone_is_refused_and_marked() {
        let tmp = synthetic_repo();
        let dir = tmp.path();
        std::fs::write(
            dir.join("auth.py"),
            "def refresh(user):\n    return token\n",
        )
        .unwrap();
        commit_all(dir, &format!("add token refresh{CLAUDE_TRAILER}"));
        let shallow = tempfile::tempdir().unwrap();
        let repo = shallow.path().join("repo");
        // `--depth` is ignored for a local path. `file://` is a real shallow clone.
        let url = format!("file://{}", dir.display());
        run_git(
            shallow.path(),
            &["clone", "-q", "--depth", "1", &url, "repo"],
        );
        let err = audit_repo(&repo, &AuditOptions::default()).unwrap_err();
        assert!(err.to_string().contains("shallow"), "{err}");
        let report = audit_repo(
            &repo,
            &AuditOptions {
                allow_shallow: true,
            },
        )
        .unwrap();
        assert!(report.coverage.shallow);
    }
}
