use std::{path::Path, process::Command};

use serde_json::Value;

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(repo)
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{args:?}: {:?}", output);
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn commit(repo: &Path, label: &str) -> String {
    git(repo, &["commit", "--allow-empty", "-m", label]);
    git(repo, &["rev-parse", "HEAD"])
}

fn handoff(repo: &Path, next: &str, expected: Option<&str>) -> (bool, Value) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_re"));
    command.current_dir(repo).args(["handoff", "review", next]);
    if let Some(expected) = expected {
        command.args(["--expect", expected]);
    }
    let output = command.output().unwrap();
    let outcome = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("{output:?}"));
    (output.status.success(), outcome)
}

#[test]
fn retry_survives_a_later_delivery_and_does_not_touch_workspace() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path();
    git(repo, &["init", "-b", "main"]);
    let base = commit(repo, "base");
    let next = commit(repo, "next");
    let tip = commit(repo, "tip");
    std::fs::write(repo.join("dirty"), "working copy").unwrap();
    let working = git(repo, &["status", "--porcelain"]);

    let (ok, first) = handoff(repo, &base, None);
    assert!(ok);
    assert_eq!(first["status"], "accepted");
    assert_eq!(handoff(repo, &next, Some(&base)).1["status"], "accepted");

    let (ok, retry) = handoff(repo, &base, None);
    assert!(ok);
    assert_eq!(retry["status"], "replayed");
    assert_eq!(retry["receipt"], first["receipt"]);
    assert_eq!(retry["current"], next);

    let (ok, stale) = handoff(repo, &tip, Some(&base));
    assert!(!ok);
    assert_eq!(stale["status"], "stale");
    assert_eq!(stale["receipt"], Value::Null);
    let (ok, backward) = handoff(repo, &base, Some(&next));
    assert!(!ok);
    assert_eq!(backward["status"], "not_fast_forward");
    assert_eq!(git(repo, &["rev-parse", "HEAD"]), tip);
    assert_eq!(git(repo, &["status", "--porcelain"]), working);
}

#[test]
fn two_git_worktrees_cannot_advance_the_same_slot_from_the_same_base() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    let worktree = temp.path().join("other");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    let base = commit(&repo, "base");
    git(
        &repo,
        &["worktree", "add", "-b", "other", worktree.to_str().unwrap()],
    );
    let a = commit(&repo, "proposal a");
    let b = commit(&worktree, "proposal b");
    assert!(handoff(&repo, &base, None).0);
    let barrier = std::sync::Barrier::new(2);
    let (a, b) = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            barrier.wait();
            handoff(&repo, &a, Some(&base)).1
        });
        barrier.wait();
        let second = handoff(&worktree, &b, Some(&base)).1;
        (first.join().unwrap(), second)
    });
    let mut statuses = [a["status"].as_str().unwrap(), b["status"].as_str().unwrap()];
    statuses.sort_unstable();
    assert_eq!(statuses, ["accepted", "stale"]);
    assert_eq!(a["current"], b["current"]);
}
