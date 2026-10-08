use anyhow::{Context, Result};
use chrono::Utc;
use colored::Colorize;
use std::io::Read;

use crate::cli::RecordArgs;
use crate::commit::{commit_event, resolve_parent, resolve_pre_snapshot};
use crate::object::{Event, Snapshot};
use crate::repo::Repo;
use crate::snapshot::snapshot_workspace;
use crate::store::Store;

/// `re record` is split in two phases for usability:
///
/// Phase 1 (pre): the agent calls `re record --pre ...` BEFORE acting.
/// Phase 2 (post): the agent calls `re record -m "..." --tool ...` AFTER acting.
///
/// For the MVP we collapse it: each call snapshots NOW as the post-state and
/// uses the previous head's post-snapshot as the pre-state. This is simpler
/// and good enough for the demo (`re revert` works correctly).
pub fn run(args: RecordArgs) -> Result<()> {
    let repo = Repo::discover()?;

    // Read the whole stdin payload BEFORE taking the lock: a slow producer
    // must never hold the repository hostage.
    let stdin_payload = if args.stdin {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("reading event JSON from stdin")?;
        Some(buf)
    } else {
        None
    };

    record_in(&repo, &args, stdin_payload.as_deref())
}

fn record_in(repo: &Repo, args: &RecordArgs, stdin_json: Option<&str>) -> Result<()> {
    // Parse and reject exit_code before the lock, the snapshot or the event.
    let stdin_value = match stdin_json {
        Some(json) => {
            Some(serde_json::from_str::<serde_json::Value>(json).context("parsing stdin JSON")?)
        }
        None => None,
    };
    let exit_code = match &stdin_value {
        Some(v) => crate::object::declared_exit_code(v)?,
        None => None,
    };

    let store = Store::new(repo);
    // Serialize the read-parent → snapshot → commit critical section against
    // other recorders (watchers, hooks, MCP calls).
    let _lock = repo.lock()?;

    let session = args.session.as_deref();
    if let Some(name) = session {
        crate::repo::validate_session_name(name)?;
    }
    let parent_id = resolve_parent(repo, session)?;
    let pre_snapshot_id = resolve_pre_snapshot(repo, &store, &parent_id)?;

    let post_tree_id = snapshot_workspace(repo)?;
    let post_snapshot = Snapshot {
        tree: post_tree_id,
        created_at: Utc::now().to_rfc3339(),
    };
    let post_snapshot_id = store.write_snapshot(&post_snapshot)?;

    // Extract metadata. CLI flags win over stdin for the same field, so
    // an agent integration can `record --stdin` and humans can `record -m "..."`.
    let mut agent = args.agent.clone();
    let mut tool = args.tool.clone();
    let mut message = args.message.clone();
    let mut model: Option<String> = None;
    let mut prompt: Option<String> = None;
    let mut reasoning: Option<String> = None;
    let mut reads: Vec<String> = Vec::new();
    let mut writes: Vec<String> = Vec::new();
    let mut tokens_in: Option<u64> = None;
    let mut tokens_out: Option<u64> = None;
    let mut cost_usd: Option<f64> = None;

    if let Some(v) = stdin_value {
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(String::from);
        let arr = |k: &str| -> Vec<String> {
            v.get(k)
                .and_then(|x| x.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        };
        agent = agent.or_else(|| s("agent"));
        tool = tool.or_else(|| s("tool"));
        message = message.or_else(|| s("message"));
        model = s("model");
        prompt = s("prompt");
        reasoning = s("reasoning");
        reads = arr("reads");
        writes = arr("writes");
        tokens_in = v.get("tokens_in").and_then(|x| x.as_u64());
        tokens_out = v.get("tokens_out").and_then(|x| x.as_u64());
        cost_usd = v.get("cost_usd").and_then(|x| x.as_f64());
    }

    let event = Event {
        schema: "causari.event.v0.2".to_string(),
        parent: parent_id.clone(),
        agent,
        model,
        tool,
        message,
        prompt,
        reasoning,
        reads,
        writes,
        tokens_in,
        tokens_out,
        cost_usd,
        pre_snapshot: pre_snapshot_id,
        post_snapshot: post_snapshot_id,
        exit_code,
        created_at: Utc::now().to_rfc3339(),
        evidence: Some(crate::object::Evidence::declared("record")),
        redactions: 0,
    };

    let event_id = commit_event(repo, &store, &event, session)?;

    let short = &event_id[..10];
    let session_note = match session {
        Some(name) => format!("  [{}]", name),
        None => String::new(),
    };
    println!(
        "{} {}  {}{}",
        "recorded".green().bold(),
        short.bright_black(),
        event.message.unwrap_or_else(|| "(no message)".to_string()),
        session_note.cyan()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args() -> RecordArgs {
        RecordArgs {
            message: None,
            tool: None,
            agent: None,
            stdin: true,
            session: None,
        }
    }

    fn stored_files(repo: &Repo) -> usize {
        fn walk(path: &std::path::Path, n: &mut usize) {
            let Ok(entries) = std::fs::read_dir(path) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, n);
                } else {
                    *n += 1;
                }
            }
        }
        let mut n = 0;
        walk(&repo.dir, &mut n);
        n
    }

    fn head_exit(repo: &Repo) -> Option<i32> {
        let id = repo.head_event().unwrap().unwrap();
        Store::new(repo).read_event(&id).unwrap().exit_code
    }

    #[test]
    fn stdin_exit_code_keeps_i32_limits_and_rejects_before_writing() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        let args = args();

        record_in(
            &repo,
            &args,
            Some(r#"{"message":"high","exit_code":2147483647}"#),
        )
        .unwrap();
        assert_eq!(head_exit(&repo), Some(i32::MAX));

        record_in(
            &repo,
            &args,
            Some(r#"{"message":"low","exit_code":-2147483648}"#),
        )
        .unwrap();
        assert_eq!(head_exit(&repo), Some(i32::MIN));

        record_in(&repo, &args, Some(r#"{"message":"absent"}"#)).unwrap();
        assert_eq!(head_exit(&repo), None);

        record_in(&repo, &args, Some(r#"{"message":"null","exit_code":null}"#)).unwrap();
        assert_eq!(head_exit(&repo), None);

        let head = repo.head_event().unwrap();
        let files = stored_files(&repo);

        let too_high = record_in(
            &repo,
            &args,
            Some(r#"{"message":"overflow","exit_code":2147483648}"#),
        )
        .unwrap_err()
        .to_string();
        assert!(
            too_high.contains("outside the signed 32-bit range"),
            "{too_high}"
        );

        let fraction = record_in(
            &repo,
            &args,
            Some(r#"{"message":"fraction","exit_code":1.5}"#),
        )
        .unwrap_err()
        .to_string();
        assert!(fraction.contains("must be an integer"), "{fraction}");

        let text = record_in(&repo, &args, Some(r#"{"message":"text","exit_code":"0"}"#))
            .unwrap_err()
            .to_string();
        assert!(text.contains("must be an integer"), "{text}");

        assert_eq!(repo.head_event().unwrap(), head);
        assert_eq!(stored_files(&repo), files);
        assert_eq!(head_exit(&repo), None);
    }
}
