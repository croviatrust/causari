use std::process::{Command, Output};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::cli::HandoffArgs;

const VERSION: &str = "causari-handoff-v1";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub slot: String,
    pub expected: Option<String>,
    pub next: String,
}

#[derive(Serialize)]
pub struct Outcome {
    status: &'static str,
    slot: String,
    expected: Option<String>,
    next: String,
    current: Option<String>,
    receipt: Option<String>,
}

pub fn run(args: HandoffArgs) -> Result<()> {
    let outcome = deliver(Request {
        slot: args.slot,
        expected: args.expect,
        next: args.next,
    })?;
    println!("{}", serde_json::to_string(&outcome)?);
    if outcome.receipt.is_none() {
        bail!("{}", outcome.status);
    }
    Ok(())
}

pub fn deliver(request: Request) -> Result<Outcome> {
    let Request {
        slot,
        expected,
        next,
    } = request;
    ensure!(
        !slot.is_empty()
            && slot.len() <= 64
            && slot.as_bytes()[0].is_ascii_lowercase()
            && slot
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
        "slot must match [a-z][a-z0-9-]{{0,63}}"
    );
    let oid_len = match git_text(&["rev-parse", "--show-object-format"])?.as_str() {
        "sha1" => 40,
        "sha256" => 64,
        _ => bail!("unsupported Git object format"),
    };
    let full_oid = |oid: &str| {
        oid.len() == oid_len
            && oid
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    ensure!(
        full_oid(&next) && expected.as_deref().is_none_or(full_oid),
        "use full lowercase Git commit IDs"
    );
    ensure!(
        expected.as_deref() != Some(&next),
        "expected and next must differ"
    );
    ensure!(
        git_text(&["cat-file", "-t", &next])? == "commit",
        "next must be a commit"
    );

    let reference = format!("refs/causari/handoff/{slot}");
    let subject = format!("{VERSION} {} {next}", expected.as_deref().unwrap_or("-"));
    let result = |status, current, receipt| Outcome {
        status,
        slot: slot.clone(),
        expected: expected.clone(),
        next: next.clone(),
        current,
        receipt,
    };

    for _ in 0..4 {
        let symref = git()
            .args(["symbolic-ref", "--quiet", &reference])
            .output()?;
        ensure!(
            symref.status.code() == Some(1),
            "handoff slot must be a direct ref"
        );
        let tip = git()
            .args(["rev-parse", "--verify", "--quiet", &reference])
            .output()?;
        let (journal, current) = if tip.status.success() {
            let journal = output_text(tip)?;
            let entry = git_text(&[
                "show",
                "-s",
                "--no-notes",
                "--no-show-signature",
                "--format=%s",
                &journal,
            ])?;
            let Some((prefix, current)) = entry.rsplit_once(' ') else {
                bail!("invalid handoff journal tip");
            };
            ensure!(
                prefix.starts_with(&format!("{VERSION} ")) && full_oid(current),
                "invalid handoff journal tip"
            );
            if entry == subject {
                return Ok(result("replayed", Some(current.to_owned()), Some(journal)));
            }
            (Some(journal), Some(current.to_owned()))
        } else {
            ensure!(tip.status.code() == Some(1), "cannot read handoff ref");
            (None, None)
        };

        if current != expected {
            if let (Some(journal), Some(current)) = (&journal, &current) {
                // The current candidate reaches all earlier candidates, so this walk
                // visits journal commits only. It runs on stale/retry paths.
                let entries = git_text(&[
                    "log",
                    "--first-parent",
                    "--no-notes",
                    "--no-show-signature",
                    "--format=%H%x09%s",
                    "--fixed-strings",
                    &format!("--grep={subject}"),
                    journal,
                    &format!("^{current}"),
                    "--",
                ])?;
                if let Some(receipt) = entries.lines().find_map(|line| {
                    let (id, entry) = line.split_once('\t')?;
                    (entry == subject).then_some(id)
                }) {
                    return Ok(result(
                        "replayed",
                        Some(current.clone()),
                        Some(receipt.to_owned()),
                    ));
                }
            }
            return Ok(result("stale", current, None));
        }
        if let Some(current) = &current {
            let ancestry = git()
                .args(["merge-base", "--is-ancestor", current, &next])
                .output()?;
            if ancestry.status.code() == Some(1) {
                return Ok(result("not_fast_forward", Some(current.clone()), None));
            }
            output_text(ancestry)?;
        }

        // One ref is the entire visibility boundary. The new journal commit
        // reaches the candidate and the old journal even after a client restart.
        let mut command = git();
        command.args([
            "commit-tree",
            &format!("{next}^{{tree}}"),
            "-m",
            &subject,
            "--no-gpg-sign",
        ]);
        if let Some(journal) = &journal {
            command.args(["-p", journal]);
        }
        command.args(["-p", &next]);
        for role in ["AUTHOR", "COMMITTER"] {
            command.env(format!("GIT_{role}_NAME"), "Causari Handoff");
            command.env(format!("GIT_{role}_EMAIL"), "handoff@causari.invalid");
            command.env_remove(format!("GIT_{role}_DATE"));
        }
        let receipt = output_text(command.output()?)?;
        let zero = "0".repeat(oid_len);
        let update = git()
            .args([
                "update-ref",
                "--no-deref",
                &reference,
                &receipt,
                journal.as_deref().unwrap_or(&zero),
            ])
            .output()?;
        if update.status.success() {
            return Ok(result("accepted", Some(next.clone()), Some(receipt)));
        }
        // A concurrent writer may have won the compare-and-swap. Re-read
        // before deciding whether this was a replay, a conflict or an error.
        if !String::from_utf8_lossy(&update.stderr).contains("cannot lock ref") {
            output_text(update)?;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    bail!("handoff slot is busy; retry the same request");
}

fn git() -> Command {
    let mut command = Command::new("git");
    command
        .arg("--no-replace-objects")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("LC_ALL", "C");
    command
}

fn git_text(args: &[&str]) -> Result<String> {
    output_text(git().args(args).output().context("starting git")?)
}

fn output_text(output: Output) -> Result<String> {
    ensure!(
        output.status.success(),
        "git: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}
