//! `re audit` end to end: the JSON contract and the shallow-clone guard,
//! exercised against real git repositories built in a temp dir.
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

/// Git with a fixed identity and no commit signing, so the user's global
/// signing setup cannot slow down or block the synthetic repositories.
fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(["-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Tarik")
        .env("GIT_AUTHOR_EMAIL", "tarik@example.com")
        .env("GIT_COMMITTER_NAME", "Tarik")
        .env("GIT_COMMITTER_EMAIL", "tarik@example.com")
        .status()
        .expect("git must be installed");
    assert!(status.success(), "git {args:?} failed");
}

fn re(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_re"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
}

fn json(out: &Output) -> serde_json::Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("valid JSON on stdout")
}

/// One human commit, then one Claude-tagged commit adding two lines.
fn repo_with_history() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    git(dir, &["init", "-q", "-b", "main"]);
    fs::write(dir.join("main.py"), "print('hello')\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", "initial scaffold"]);
    fs::write(
        dir.join("auth.py"),
        "def refresh(user):\n    return rotate(user)\n",
    )
    .unwrap();
    git(dir, &["add", "."]);
    git(
        dir,
        &[
            "commit",
            "-q",
            "-m",
            "add token refresh\n\nCo-Authored-By: Claude <noreply@anthropic.com>",
        ],
    );
    temp
}

#[test]
fn json_report_carries_compat_fields_and_method_extras() {
    let temp = repo_with_history();
    let v = json(&re(temp.path(), &["audit", "--json"]));

    assert_eq!(v["total_commits"], 2);
    assert_eq!(v["verified"]["commits"], 1);
    assert_eq!(v["verified"]["introduced"], 2);
    assert_eq!(v["verified"]["surviving"], 2);
    assert_eq!(v["verified"]["survival_rate"], 1.0);
    assert_eq!(v["probable"]["commits"], 0);
    assert_eq!(v["by_agent"]["claude-code"]["introduced"], 2);

    for key in [
        "median_survival",
        "capped_survival_rate",
        "largest_commit_share",
    ] {
        assert!(v["verified"].get(key).is_some(), "verified.{key} missing");
        assert!(
            v["by_agent"]["claude-code"].get(key).is_some(),
            "by_agent.claude-code.{key} missing"
        );
    }
    assert_eq!(v["coverage"]["method"], "v4");
    assert_eq!(
        v["coverage"]["blame_flags"],
        serde_json::json!(["-w", "-M", "-C"])
    );
    assert_eq!(v["coverage"]["shallow"], false);
    assert_eq!(v["coverage"]["sample_floor"], 5);
    assert_eq!(v["coverage"]["small_sample"], true);
    assert_eq!(v["method"], "v4");

    // Method v3: the same repository's untagged lines are the baseline. The
    // scaffold commit is the one untagged commit; its line stands at HEAD.
    let b = &v["baseline"];
    assert_eq!(b["untagged"]["commits"], 1);
    assert_eq!(b["untagged"]["introduced"], 1);
    assert_eq!(b["untagged"]["surviving"], 1);
    assert_eq!(b["by_age"].as_array().map(Vec::len), Some(6));
    assert_eq!(b["by_age"][0]["from_days"], 0);
    assert_eq!(b["by_age"][0]["to_days"], 30);
    assert_eq!(b["by_age"][0]["tagged"]["commits"], 1);
    assert_eq!(b["by_age"][0]["untagged"]["commits"], 1);
    assert!(b["by_age"][5]["to_days"].is_null());
    // Two commits do not reach the floor: no age-matched figure, and the
    // oldest surviving line is the first commit with nothing before it.
    assert!(b["age_matched"].is_null());
    assert_eq!(b["oldest_surviving"]["commits_before"], 0);
    assert_eq!(b["oldest_surviving"]["age_days"], 0);

    // The report names what it measured: the commit at HEAD, and the origin
    // label (a digest of the path here, since this repo has no remote).
    let head = v["repository"]["head"].as_str().expect("repository.head");
    assert_eq!(head.len(), 40);
    assert!(head.chars().all(|c| c.is_ascii_hexdigit()));
    let origin = v["repository"]["origin"]
        .as_str()
        .expect("repository.origin");
    assert!(
        origin.starts_with("sha256:"),
        "origin without remote: {origin}"
    );
}

#[test]
fn human_readable_outputs_name_the_method_version_and_no_verdict() {
    let temp = repo_with_history();
    let summary = re(temp.path(), &["audit", "--summary"]);
    assert!(summary.status.success());
    let text = String::from_utf8_lossy(&summary.stdout).into_owned();
    let sub = text
        .lines()
        .find(|l| l.starts_with("<sub>"))
        .expect("summary ends with a <sub> footer");
    assert!(sub.contains("Method v4"), "{sub}");
    assert!(text.contains("untagged lines"), "{text}");
    assert!(text.contains("capped") && text.contains("median"), "{text}");

    let terminal = re(temp.path(), &["audit"]);
    assert!(terminal.status.success());
    let text = String::from_utf8_lossy(&terminal.stdout).into_owned() + &text;
    assert!(text.contains("method v4"), "{text}");
    assert!(text.contains("Baseline: untagged lines"), "{text}");
    // Hard rule of the project: audit output measures, it does not grade.
    for verdict in ["healthy", "churn", "waste", "🟢", "🟡", "🔴"] {
        assert!(
            !text.to_lowercase().contains(verdict),
            "verdict word {verdict:?} in audit output"
        );
    }
}

#[test]
fn shallow_clone_is_refused_unless_allowed() {
    let origin = repo_with_history();
    let clones = tempfile::tempdir().unwrap();
    let clone = clones.path().join("shallow");
    let url = format!("file://{}", origin.path().display());
    git(
        clones.path(),
        &["clone", "-q", "--depth", "1", &url, clone.to_str().unwrap()],
    );

    let refused = re(&clone, &["audit", "--json"]);
    assert!(!refused.status.success(), "a shallow clone must be refused");
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(stderr.contains("shallow"), "{stderr}");
    assert!(stderr.contains("git fetch --unshallow"), "{stderr}");
    assert!(stderr.contains("fetch-depth: 0"), "{stderr}");
    assert!(stderr.contains("--allow-shallow"), "{stderr}");
    assert!(refused.stdout.is_empty(), "no partial report on refusal");

    let allowed = re(&clone, &["audit", "--json", "--allow-shallow"]);
    let stderr = String::from_utf8_lossy(&allowed.stderr);
    assert!(
        stderr.contains("warning") && stderr.contains("shallow"),
        "{stderr}"
    );
    let v = json(&allowed);
    assert_eq!(v["coverage"]["shallow"], true);
    assert_eq!(v["coverage"]["method"], "v4");
}

fn commit_file(dir: &Path, name: &str, body: &str, message: &str) {
    fs::write(dir.join(name), body).unwrap();
    git(dir, &["add", name]);
    git(dir, &["commit", "-q", "-m", message]);
}

/// Five untagged commits and five metadata-matched commits in one age
/// window, plus one Cursor commit so one agent row is under the floor.
fn repo_with_age_match_and_small_agent() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    git(dir, &["init", "-q", "-b", "main"]);
    for i in 0..5 {
        commit_file(
            dir,
            &format!("hand{i}.py"),
            &format!("hand = {i}\n"),
            &format!("hand {i}"),
        );
    }
    for i in 0..5 {
        commit_file(
            dir,
            &format!("ai{i}.py"),
            &format!("ai = {i}\n"),
            &format!("ai {i}\n\nCo-Authored-By: Claude <noreply@anthropic.com>"),
        );
    }
    commit_file(
        dir,
        "cursor.py",
        "cursor = 1\n",
        "cursor\n\nCo-Authored-By: Cursor Agent <cursoragent@cursor.com>",
    );
    temp
}

#[test]
fn audit_reading_export_and_unavailable_age_match() {
    let temp = repo_with_history();
    let terminal = re(temp.path(), &["audit"]);
    assert!(terminal.status.success());
    let text = String::from_utf8_lossy(&terminal.stdout);
    let survival = text.find("survival ").expect("survival line");
    let meaning = text
        .find("not the share of the repository")
        .expect("meaning");
    let gap = text.find("Age-matched gap: unavailable").expect("gap");
    let probable = text.find("Probable AI-assisted").expect("probable");
    assert!(
        survival < meaning && meaning < gap && gap < probable,
        "reading order:\n{text}"
    );
    let line_after_survival = text[survival..].lines().nth(1).unwrap_or("");
    assert!(
        line_after_survival.contains("not the share of the repository"),
        "meaning must be the next line after the percentage, got {line_after_survival:?}"
    );
    assert!(
        text.contains("does not prove who wrote each line"),
        "{text}"
    );
    assert!(
        text.contains("not a finding that a human wrote them"),
        "{text}"
    );
    assert!(
        text.contains("below floor: 1 commit (floor 5); not comparable"),
        "{text}"
    );
    assert!(
        text.contains("Meeting the floor is not a reliability guarantee"),
        "{text}"
    );
    assert!(text.contains("--json"), "{text}");
    assert!(text.contains("--summary"), "{text}");
    assert!(text.contains("--seal"), "{text}");
    assert!(text.contains("refs/notes/ai"), "{text}");
    assert!(text.contains("has moved"), "{text}");
    assert!(
        text.contains("`re report` reads the local ledger"),
        "{text}"
    );

    let summary = re(temp.path(), &["audit", "--summary"]);
    assert!(summary.status.success());
    let summary_text = String::from_utf8_lossy(&summary.stdout);
    let bold = summary_text
        .find("AI-tagged (metadata matched)")
        .expect("summary headline");
    let summary_gap = summary_text
        .find("Age-matched gap: unavailable")
        .expect("summary gap");
    assert!(bold < summary_gap, "{summary_text}");
    assert!(
        summary_text.contains("claude-code: below floor:"),
        "{summary_text}"
    );

    let exported = json(&re(temp.path(), &["audit", "--json"]));
    let head = exported["repository"]["head"]
        .as_str()
        .expect("repository.head");
    assert_eq!(head.len(), 40);
    assert!(
        text.contains(head),
        "terminal names the measured commit:\n{text}"
    );
    assert_eq!(exported["coverage"]["method"], "v4");
    assert_eq!(
        exported["coverage"]["blame_flags"],
        serde_json::json!(["-w", "-M", "-C"])
    );
    assert!(exported["baseline"]["age_matched"].is_null());
}

#[test]
fn age_matched_gap_states_units_and_direction() {
    let temp = repo_with_age_match_and_small_agent();
    let terminal = re(temp.path(), &["audit"]);
    assert!(terminal.status.success());
    let text = String::from_utf8_lossy(&terminal.stdout);
    assert!(
        text.contains(
            "Age-matched gap: +0.0 percentage points (AI-tagged 100.0% minus untagged 100.0% in the matched age windows)"
        ),
        "{text}"
    );
    assert!(
        !text.contains("same age") && !text.contains("of the same age"),
        "windows are buckets, not identical timestamps:\n{text}"
    );
    assert!(
        text.contains(
            "Positive means the metadata-matched lines have the higher line-weighted survival"
        ),
        "{text}"
    );
    assert!(text.contains("below floor: 1 commit (floor 5)"), "{text}");
    // The five-commit agent is at the floor. The warning belongs to the
    // one-commit row. Meeting the floor is not described as reliability.
    let cursor = text.find("cursor").expect("cursor row");
    let claude = text.find("claude-code").expect("claude row");
    let cursor_warning = text[cursor..].find("below floor").expect("cursor floor");
    let claude_section_end = if claude < cursor { cursor } else { text.len() };
    let claude_slice = if claude < cursor {
        &text[claude..claude_section_end]
    } else {
        &text[claude..]
    };
    assert!(
        !claude_slice.contains("below floor"),
        "claude row is at the floor and must not be marked below it:\n{claude_slice}"
    );
    assert!(
        cursor_warning < 400,
        "cursor warning follows the cursor row"
    );

    let v = json(&re(temp.path(), &["audit", "--json"]));
    let gap = v["baseline"]["age_matched"]["gap"].as_f64().expect("gap");
    assert!(gap.abs() < 1e-9, "{gap}");
    assert_eq!(v["by_agent"]["claude-code"]["commits"], 5);
    assert_eq!(v["by_agent"]["cursor"]["commits"], 1);
}

#[test]
fn same_commit_changes_class_when_the_git_ai_note_changes() {
    let temp = repo_with_history();
    let dir = temp.path();
    let before = json(&re(dir, &["audit", "--json"]));
    let head = before["repository"]["head"].as_str().unwrap().to_string();
    assert_eq!(before["verified"]["commits"], 1);

    let schema_only = dir.join("schema-only.txt");
    fs::write(
        &schema_only,
        "auth.py\n---\n{\"schema_version\":\"authorship/3.0.0\",\"prompts\":{}}\n",
    )
    .unwrap();
    git(
        dir,
        &[
            "notes",
            "--ref=ai",
            "add",
            "-f",
            "-F",
            schema_only.to_str().unwrap(),
            "HEAD",
        ],
    );
    let mid = json(&re(dir, &["audit", "--json"]));
    assert_eq!(mid["repository"]["head"], head);
    assert_eq!(
        mid["verified"]["commits"], 1,
        "a schema-only note must not add a verified commit under v4"
    );

    let named = dir.join("named.txt");
    fs::write(
        &named,
        concat!(
            "auth.py\n  p 1\n---\n",
            "{\"schema_version\":\"authorship/3.0.0\",",
            "\"prompts\":{\"p\":{\"agent_id\":{\"tool\":\"mock_ai\"}}}}\n"
        ),
    )
    .unwrap();
    // HEAD is the Claude-tagged commit. A named tool in the note outranks
    // the trailer, so the agent changes while the commit SHA does not.
    git(
        dir,
        &[
            "notes",
            "--ref=ai",
            "add",
            "-f",
            "-F",
            named.to_str().unwrap(),
            "HEAD",
        ],
    );
    let after = json(&re(dir, &["audit", "--json"]));
    assert_eq!(after["repository"]["head"], head);
    assert!(
        after["by_agent"].get("mock_ai").is_some(),
        "named tool on an unchanged commit changes the agent: {after}"
    );
    // The agent key is the classification result. The note body is not stored.
    let raw = serde_json::to_string(&after).unwrap();
    assert!(!raw.contains("schema_version"), "{raw}");
    assert!(!raw.contains("authorship/3.0.0"), "{raw}");
    assert!(after.get("notes").is_none());
    assert!(after["repository"].get("notes").is_none());
}

#[test]
fn report_outside_a_ledger_points_at_remote_audit() {
    let temp = tempfile::tempdir().unwrap();
    let out = re(temp.path(), &["report", "--open"]);
    assert!(!out.status.success(), "report outside a ledger must fail");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("local ledger"), "{err}");
    assert!(err.contains("re init"), "{err}");
    assert!(err.contains("re audit owner/repo"), "{err}");
    assert!(
        !err.contains("web audit"),
        "no unimplemented web flow: {err}"
    );
    assert!(err.contains("uses the local ledger"), "{err}");
}

const HEAD_TS: i64 = 1_700_000_000;
const DAY: i64 = 86_400;

fn git_at(dir: &Path, args: &[&str], unix: i64) {
    let when = unix.to_string();
    let status = Command::new("git")
        .args(["-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Tarik")
        .env("GIT_AUTHOR_EMAIL", "tarik@example.com")
        .env("GIT_COMMITTER_NAME", "Tarik")
        .env("GIT_COMMITTER_EMAIL", "tarik@example.com")
        .env("GIT_AUTHOR_DATE", &when)
        .env("GIT_COMMITTER_DATE", &when)
        .status()
        .expect("git must be installed");
    assert!(status.success(), "git {args:?} failed");
}

fn commit_dated(dir: &Path, name: &str, body: &str, message: &str, unix: i64) {
    fs::write(dir.join(name), body).unwrap();
    git_at(dir, &["add", "--", name], unix);
    git_at(dir, &["commit", "-q", "-m", message], unix);
}

/// Five untagged and five Claude commits in the 0–30 day window. `positive`
/// then deletes one line from each untagged file; otherwise from each tagged
/// file. The deletion is a later untagged commit with no introduced lines.
fn repo_signed_gap(positive: bool) -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let young = HEAD_TS - 10 * DAY;
    git_at(dir, &["init", "-q", "-b", "main"], young);
    for i in 0..5 {
        commit_dated(
            dir,
            &format!("hand{i}.py"),
            "h0\nh1\n",
            &format!("hand {i}"),
            young,
        );
        commit_dated(
            dir,
            &format!("ai{i}.py"),
            "a0\na1\n",
            &format!("ai {i}\n\nCo-Authored-By: Claude <noreply@anthropic.com>"),
            young,
        );
    }
    for i in 0..5 {
        if positive {
            fs::write(dir.join(format!("hand{i}.py")), "h0\n").unwrap();
        } else {
            fs::write(dir.join(format!("ai{i}.py")), "a0\n").unwrap();
        }
    }
    git_at(dir, &["add", "."], HEAD_TS);
    git_at(
        dir,
        &[
            "commit",
            "-q",
            "-m",
            if positive {
                "trim untagged"
            } else {
                "trim tagged"
            },
        ],
        HEAD_TS,
    );
    temp
}

struct ExpectedGap {
    sentence: &'static str,
    gap: f64,
    tagged: f64,
    untagged: f64,
    covered: f64,
}

fn assert_gap_pair(text: &str, summary: &str, v: &serde_json::Value, expected: ExpectedGap) {
    assert!(text.contains(expected.sentence), "terminal:\n{text}");
    assert!(summary.contains(expected.sentence), "summary:\n{summary}");
    assert!(
        !text.contains("same age"),
        "terminal must not claim identical ages:\n{text}"
    );
    let m = &v["baseline"]["age_matched"];
    let got_gap = m["gap"].as_f64().expect("gap");
    let got_tagged = m["tagged_rate"].as_f64().expect("tagged_rate");
    let got_untagged = m["untagged_rate"].as_f64().expect("untagged_rate");
    let got_covered = m["tagged_lines_covered"].as_f64().expect("covered");
    assert!(
        (got_gap - expected.gap).abs() < 1e-9,
        "gap {got_gap} != {}",
        expected.gap
    );
    assert!(
        (got_tagged - expected.tagged).abs() < 1e-9,
        "tagged {got_tagged}"
    );
    assert!(
        (got_untagged - expected.untagged).abs() < 1e-9,
        "untagged {got_untagged}"
    );
    assert!(
        (got_covered - expected.covered).abs() < 1e-9,
        "covered {got_covered}"
    );
    assert_eq!(m["buckets_used"], 1);
    // JSON keeps the rate. The sentence converts it to percentage points.
    assert!((got_gap * 100.0 - expected.gap * 100.0).abs() < 1e-9);
    assert!(v.get("notes").is_none());
}

#[test]
fn age_matched_gap_sign_and_percentage_points_match_json() {
    let positive_sentence = "Age-matched gap: +50.0 percentage points (AI-tagged 100.0% minus untagged 50.0% in the matched age windows)";
    let pos = repo_signed_gap(true);
    let pos_out = re(pos.path(), &["audit"]);
    assert!(
        pos_out.status.success(),
        "{}",
        String::from_utf8_lossy(&pos_out.stderr)
    );
    let pos_sum = re(pos.path(), &["audit", "--summary"]);
    assert!(
        pos_sum.status.success(),
        "{}",
        String::from_utf8_lossy(&pos_sum.stderr)
    );
    let pos_text = String::from_utf8_lossy(&pos_out.stdout).into_owned();
    let pos_summary = String::from_utf8_lossy(&pos_sum.stdout).into_owned();
    let pos_json = json(&re(pos.path(), &["audit", "--json"]));
    assert_gap_pair(
        &pos_text,
        &pos_summary,
        &pos_json,
        ExpectedGap {
            sentence: positive_sentence,
            gap: 0.5,
            tagged: 1.0,
            untagged: 0.5,
            covered: 1.0,
        },
    );
    assert!(
        pos_text.contains("holding 100% of the AI-tagged lines"),
        "{pos_text}"
    );
    assert!((pos_json["verified"]["survival_rate"].as_f64().unwrap() - 1.0).abs() < 1e-9);

    let negative_sentence = "Age-matched gap: -50.0 percentage points (AI-tagged 50.0% minus untagged 100.0% in the matched age windows)";
    let neg = repo_signed_gap(false);
    let neg_out = re(neg.path(), &["audit"]);
    assert!(
        neg_out.status.success(),
        "{}",
        String::from_utf8_lossy(&neg_out.stderr)
    );
    let neg_sum = re(neg.path(), &["audit", "--summary"]);
    assert!(
        neg_sum.status.success(),
        "{}",
        String::from_utf8_lossy(&neg_sum.stderr)
    );
    let neg_text = String::from_utf8_lossy(&neg_out.stdout).into_owned();
    let neg_summary = String::from_utf8_lossy(&neg_sum.stdout).into_owned();
    let neg_json = json(&re(neg.path(), &["audit", "--json"]));
    assert_gap_pair(
        &neg_text,
        &neg_summary,
        &neg_json,
        ExpectedGap {
            sentence: negative_sentence,
            gap: -0.5,
            tagged: 0.5,
            untagged: 1.0,
            covered: 1.0,
        },
    );
    assert!(
        neg_text.contains("holding 100% of the AI-tagged lines"),
        "{neg_text}"
    );
    assert!((neg_json["verified"]["survival_rate"].as_f64().unwrap() - 0.5).abs() < 1e-9);
}

/// Tagged lines in a 90–180 day window have no untagged counterpart, so they
/// stay out of the gap. The 0–30 day window is the positive case above.
#[test]
fn age_matched_gap_covers_only_comparable_windows() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let old = HEAD_TS - 100 * DAY;
    let young = HEAD_TS - 10 * DAY;
    git_at(dir, &["init", "-q", "-b", "main"], old);
    for i in 0..5 {
        commit_dated(
            dir,
            &format!("old{i}.py"),
            "o0\no1\n",
            &format!("old {i}\n\nCo-Authored-By: Claude <noreply@anthropic.com>"),
            old,
        );
    }
    for i in 0..5 {
        commit_dated(
            dir,
            &format!("hand{i}.py"),
            "h0\nh1\n",
            &format!("hand {i}"),
            young,
        );
        commit_dated(
            dir,
            &format!("ai{i}.py"),
            "a0\na1\n",
            &format!("young {i}\n\nCo-Authored-By: Claude <noreply@anthropic.com>"),
            young,
        );
    }
    for i in 0..5 {
        fs::remove_file(dir.join(format!("old{i}.py"))).unwrap();
        fs::write(dir.join(format!("hand{i}.py")), "h0\n").unwrap();
    }
    git_at(dir, &["add", "-A"], HEAD_TS);
    git_at(
        dir,
        &["commit", "-q", "-m", "drop old and trim young untagged"],
        HEAD_TS,
    );

    let out = re(dir, &["audit"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let summary = String::from_utf8_lossy(&re(dir, &["audit", "--summary"]).stdout).into_owned();
    let v = json(&re(dir, &["audit", "--json"]));
    let sentence = "Age-matched gap: +50.0 percentage points (AI-tagged 100.0% minus untagged 50.0% in the matched age windows)";
    assert_gap_pair(
        &text,
        &summary,
        &v,
        ExpectedGap {
            sentence,
            gap: 0.5,
            tagged: 1.0,
            untagged: 0.5,
            covered: 0.5,
        },
    );
    assert!(
        text.contains("holding 50% of the AI-tagged lines"),
        "{text}"
    );
    assert!(
        text.contains("Lines outside those windows are not in this gap."),
        "{text}"
    );
    // Headline survival counts the deleted older lines. The gap does not.
    let headline = v["verified"]["survival_rate"].as_f64().unwrap();
    assert!((headline - 0.5).abs() < 1e-9, "headline {headline}");
    assert!(
        (v["baseline"]["age_matched"]["tagged_rate"]
            .as_f64()
            .unwrap()
            - headline)
            .abs()
            > 0.4,
        "windowed tagged rate must differ from the headline when coverage is partial"
    );
    assert_eq!(v["verified"]["introduced"], 20);
    assert_eq!(v["verified"]["surviving"], 10);
}

#[test]
fn ledger_commands_share_the_discover_error() {
    let temp = tempfile::tempdir().unwrap();
    let cmds: &[&[&str]] = &[
        &["log"],
        &["why", "file.py:1"],
        &["churn"],
        &["record", "-m", "note"],
        &["seal", "list"],
        &["report"],
        &["show", "abcdef"],
        &["diff", "abc"],
    ];
    for args in cmds {
        let out = re(temp.path(), args);
        assert!(
            !out.status.success(),
            "{args:?} should fail without a ledger"
        );
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("uses the local ledger"),
            "{args:?} must use the shared discover error:\n{err}"
        );
        assert!(err.contains("`re init` creates it"), "{args:?}: {err}");
        assert!(err.contains("re audit owner/repo"), "{args:?}: {err}");
        assert!(
            err.contains("does not audit a remote repository"),
            "{args:?}: {err}"
        );
    }

    let audit = re(temp.path(), &["audit"]);
    let err = String::from_utf8_lossy(&audit.stderr);
    assert!(
        !err.contains("local ledger"),
        "audit does not require the ledger:\n{err}"
    );
}
