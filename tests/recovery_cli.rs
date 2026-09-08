use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
fn re(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_re"))
        .args(args)
        .current_dir(root)
        .output()
        .unwrap()
}
fn ok(root: &Path, args: &[&str]) -> Output {
    let output = re(root, args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
fn record(root: &Path, state: &str) -> String {
    fs::write(root.join("state"), state).unwrap();
    ok(root, &["record", "-m", state]);
    fs::read_to_string(root.join(".causari/refs/sessions/main"))
        .unwrap()
        .trim()
        .to_string()
}
fn fixture() -> (tempfile::TempDir, String, String) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    ok(root, &["init"]);
    let good = record(root, "good");
    let bad = record(root, "bad");
    fs::write(root.join("state"), "unrecorded edit").unwrap();
    fs::write(root.join("personal-notes"), "keep me").unwrap();
    (temp, good, bad)
}
fn assert_preserved(root: &Path) {
    assert_eq!(
        fs::read_to_string(root.join("state")).unwrap(),
        "unrecorded edit"
    );
    assert_eq!(
        fs::read_to_string(root.join("personal-notes")).unwrap(),
        "keep me"
    );
}
#[test]
fn revert_dry_run_is_non_mutating_and_reports_deletions() {
    let (temp, _, bad) = fixture();
    let output = ok(temp.path(), &["revert", &bad, "--dry-run"]);
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 deleted"));
    assert_preserved(temp.path());
    assert_eq!(
        fs::read_to_string(temp.path().join(".causari/refs/sessions/main"))
            .unwrap()
            .trim(),
        bad
    );
}
#[cfg(unix)]
#[test]
fn bisect_finds_real_regression_and_preserves_dirty_workspace() {
    let (temp, good, bad) = fixture();
    let output = ok(
        temp.path(),
        &[
            "bisect",
            "--good",
            &good,
            "--bad",
            &bad,
            "--test",
            "test \"$(cat state)\" = good",
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&format!("first bad event: {bad}")));
    assert_preserved(temp.path());
}
#[test]
fn bisect_rejects_invalid_endpoints_and_execution_errors_without_data_loss() {
    for cmd in ["exit 0", "exit 1", "exit 125", "exit 126", "exit 127"] {
        let (temp, good, bad) = fixture();
        let output = re(
            temp.path(),
            &["bisect", "--good", &good, "--bad", &bad, "--test", cmd],
        );
        assert!(!output.status.success(), "accepted invalid command: {cmd}");
        assert!(!String::from_utf8_lossy(&output.stdout).contains("first bad event:"));
        assert_preserved(temp.path());
    }
}
