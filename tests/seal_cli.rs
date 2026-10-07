//! `re audit --seal` and `re seal verify` end to end: the bundle a stranger
//! receives verifies with the binary alone, from any directory, and every
//! alteration is caught with the documented exit status.
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

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
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

fn head(dir: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn audit_seal_bundle_is_written_and_verifies_from_anywhere() {
    let temp = repo_with_history();
    let dir = temp.path();

    // --json --seal: stdout carries exactly the sealed bytes.
    let out = re(dir, &["audit", "--json", "--seal", "-o", "first.seal.json"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let bundle_text = fs::read_to_string(dir.join("first.seal.json")).unwrap();
    let bundle: serde_json::Value = serde_json::from_str(&bundle_text).unwrap();
    assert_eq!(bundle["bundle"], "causari.audit-seal.v1");
    assert_eq!(
        bundle["subject"]["audit_json"].as_str().unwrap(),
        stdout(&out)
    );
    assert_eq!(bundle["subject"]["input"]["commit"], head(dir));
    assert_eq!(bundle["subject"]["input"]["method"], "v4");
    assert_eq!(bundle["seal"]["seal_version"], "crovia.seal.v1");
    assert_eq!(bundle["seal"]["generator"]["id"], "causari");
    assert_eq!(bundle["seal"]["generator"]["params"]["commit"], head(dir));
    assert_eq!(
        bundle["seal"]["generator"]["params"]["coverage.shallow"],
        "false"
    );
    assert_eq!(bundle["seal"]["subject"]["modality"], "text");
    assert_eq!(bundle["seal"]["chain"]["sequence"], 0);
    assert!(stderr(&out).contains("written to first.seal.json"));
    // The issuer identity lives in the audited repository, gitignored.
    assert!(dir.join(".causari/keys/seal-issuer.key").is_file());
    assert!(dir.join(".causari/seal/seals.jsonl").is_file());
    assert!(
        fs::read_to_string(dir.join(".gitignore"))
            .unwrap()
            .contains(".causari/")
    );

    // Default file name, terminal output, chain advances.
    let out = re(dir, &["audit", "--seal"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("written to audit.seal.json"), "{text}");
    assert!(text.contains("re seal verify audit.seal.json"), "{text}");
    let second: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.join("audit.seal.json")).unwrap()).unwrap();
    assert_eq!(second["seal"]["chain"]["sequence"], 1);
    assert!(second["seal"]["chain"]["prev_seal_hash"].is_string());

    // Verify from a directory with no repository at all.
    let elsewhere = tempfile::tempdir().unwrap();
    let copy = elsewhere.path().join("received.seal.json");
    fs::write(&copy, &bundle_text).unwrap();
    let out = re(
        elsewhere.path(),
        &["seal", "verify", copy.to_str().unwrap()],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("signature valid"), "{text}");
    assert!(text.contains(&head(dir)), "{text}");
    assert!(text.contains("method    v4"), "{text}");
    assert!(text.contains("does not prove they are true"), "{text}");

    let out = re(
        elsewhere.path(),
        &["seal", "verify", "--json", copy.to_str().unwrap()],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["valid"], true);
    assert_eq!(v["kind"], "audit");
    assert_eq!(v["commit"], head(dir));
    assert_eq!(v["method"], "v4");
    assert_eq!(v["sequence"], 0);
    assert_eq!(v["audit"]["verified"]["surviving"], 2);
    assert_eq!(
        v["pubkey_hex"],
        bundle["seal"]["issuer"]["pubkey"]["key_hex"]
    );

    // The repository chain holds both audit seals.
    let out = re(dir, &["seal", "verify"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("2 seal(s) verified"));
    let out = re(dir, &["seal", "list"]);
    assert!(stdout(&out).contains("audit @"), "{}", stdout(&out));
}

#[test]
fn altered_bundles_exit_1_and_garbage_exits_2() {
    let temp = repo_with_history();
    let dir = temp.path();
    let out = re(dir, &["audit", "--seal", "--json"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = fs::read_to_string(dir.join("audit.seal.json")).unwrap();
    let bundle: serde_json::Value = serde_json::from_str(&text).unwrap();

    let elsewhere = tempfile::tempdir().unwrap();
    let check = |name: &str, value: &serde_json::Value| -> Output {
        let path = elsewhere.path().join(name);
        fs::write(&path, serde_json::to_string_pretty(value).unwrap()).unwrap();
        re(
            elsewhere.path(),
            &["seal", "verify", "--json", path.to_str().unwrap()],
        )
    };

    // Numbers edited: output hash mismatch.
    let mut b = bundle.clone();
    let audit = b["subject"]["audit_json"].as_str().unwrap().to_string();
    b["subject"]["audit_json"] =
        serde_json::Value::String(audit.replace("\"surviving\": 2", "\"surviving\": 0"));
    let out = check("numbers.json", &b);
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["valid"], false);
    assert!(
        v["reason"]
            .as_str()
            .unwrap()
            .contains("numbers were altered"),
        "{v}"
    );

    // Commit edited: input hash mismatch.
    let mut b = bundle.clone();
    b["subject"]["input"]["commit"] = serde_json::Value::String("0".repeat(40));
    let out = check("commit.json", &b);
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["reason"].as_str().unwrap().contains("input_hash"), "{v}");

    // Signed field edited: signature fails. Human-readable output too.
    let mut b = bundle.clone();
    b["seal"]["generator"]["params"]["method"] = serde_json::Value::String("v9".into());
    let path = elsewhere.path().join("method.json");
    fs::write(&path, serde_json::to_string(&b).unwrap()).unwrap();
    let out = re(
        elsewhere.path(),
        &["seal", "verify", path.to_str().unwrap()],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("invalid:"), "{}", stdout(&out));
    assert!(stdout(&out).contains("signature"), "{}", stdout(&out));

    // Not a seal at all.
    let path = elsewhere.path().join("garbage.json");
    fs::write(&path, "not json").unwrap();
    let out = re(
        elsewhere.path(),
        &["seal", "verify", path.to_str().unwrap()],
    );
    assert_eq!(out.status.code(), Some(2));
    let out = re(
        elsewhere.path(),
        &["seal", "verify", "/nonexistent/file.json"],
    );
    assert_eq!(out.status.code(), Some(2));

    // The bare seal inside the bundle verifies on its own.
    let out = check("bare.json", &bundle["seal"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["kind"], "seal");
}

#[test]
fn summary_mode_ends_with_a_markdown_seal_note() {
    let temp = repo_with_history();
    let dir = temp.path();
    let out = re(dir, &["audit", "--summary", "--seal", "-o", "ci.seal.json"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let md = stdout(&out);
    let bundle: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.join("ci.seal.json")).unwrap()).unwrap();
    let seal_id = bundle["seal"]["seal_id"].as_str().unwrap();
    let note = md.lines().last().unwrap();
    assert!(note.starts_with("Sealed: `"), "{md}");
    assert!(note.contains(seal_id), "{md}");
    assert!(note.contains(&head(dir)[..12]), "{md}");
    assert!(note.contains("`re seal verify ci.seal.json`"), "{md}");
    assert!(note.contains("causari.dev/verify"), "{md}");
    assert!(note.contains("not that they are true"), "{md}");
    // The terminal block stays out of the Markdown.
    assert!(!md.contains("issuer   urn:"), "{md}");
    assert!(!md.contains("\u{1b}["), "no ANSI in Markdown: {md}");
}

#[test]
fn shallow_clone_is_refused_with_exit_2_and_sealed_only_when_allowed() {
    let origin = repo_with_history();
    let clones = tempfile::tempdir().unwrap();
    let clone = clones.path().join("shallow");
    let url = format!("file://{}", origin.path().display());
    git(
        clones.path(),
        &["clone", "-q", "--depth", "1", &url, clone.to_str().unwrap()],
    );

    let refused = re(&clone, &["audit", "--seal"]);
    assert_eq!(refused.status.code(), Some(2));
    assert!(stderr(&refused).contains("shallow"));
    assert!(!clone.join("audit.seal.json").exists());

    let out = re(&clone, &["audit", "--seal", "--allow-shallow", "--summary"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let bundle: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(clone.join("audit.seal.json")).unwrap()).unwrap();
    assert_eq!(
        bundle["seal"]["generator"]["params"]["coverage.shallow"],
        "true"
    );
    assert_eq!(bundle["subject"]["input"]["options"]["allow_shallow"], true);
    // A clone's origin URL is the repo label; no local path leaks.
    assert_eq!(
        bundle["seal"]["generator"]["params"]["repo"]
            .as_str()
            .unwrap(),
        url
    );

    let out = re(
        clones.path(),
        &["seal", "verify", "shallow/audit.seal.json"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("shallow clone"), "{}", stdout(&out));
}

#[test]
fn re_proof_is_retired_with_exit_2() {
    let temp = repo_with_history();
    for args in [
        &["proof"][..],
        &["proof", "generate"][..],
        &["proof", "verify", "--against-repo", "x.json"][..],
    ] {
        let out = re(temp.path(), args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let text = stderr(&out);
        assert!(text.contains("retired"), "{text}");
        assert!(text.contains("re audit --seal"), "{text}");
        assert!(text.contains("re seal verify"), "{text}");
    }
    // Gone from the help.
    let out = re(temp.path(), &["--help"]);
    assert!(!stdout(&out).contains("proof"), "{}", stdout(&out));
}
