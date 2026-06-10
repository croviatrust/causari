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
    let store = Store::new(&repo);

    // Serialize the read-parent → snapshot → commit critical section against
    // other recorders (watchers, hooks, MCP calls).
    let _lock = repo.lock()?;

    let session = args.session.as_deref();
    let parent_id = resolve_parent(&repo, session)?;
    let pre_snapshot_id = resolve_pre_snapshot(&repo, &store, &parent_id)?;

    let post_tree_id = snapshot_workspace(&repo)?;
    let post_snapshot = Snapshot {
        tree: post_tree_id,
        created_at: Utc::now().to_rfc3339(),
    };
    let post_snapshot_id = store.write_snapshot(&post_snapshot)?;

    // Optionally read full JSON payload from stdin.
    let stdin_payload = if args.stdin {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("reading event JSON from stdin")?;
        Some(buf)
    } else {
        None
    };

    // Extract metadata. CLI flags win over stdin for the same field, so
    // an agent integration can `record --stdin` and humans can `record -m "..."`.
    let mut agent = args.agent;
    let mut tool = args.tool;
    let mut message = args.message;
    let mut model: Option<String> = None;
    let mut prompt: Option<String> = None;
    let mut reasoning: Option<String> = None;
    let mut reads: Vec<String> = Vec::new();
    let mut writes: Vec<String> = Vec::new();
    let mut tokens_in: Option<u64> = None;
    let mut tokens_out: Option<u64> = None;
    let mut cost_usd: Option<f64> = None;
    let mut exit_code: Option<i32> = None;

    if let Some(json) = stdin_payload {
        let v: serde_json::Value = serde_json::from_str(&json).context("parsing stdin JSON")?;
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
        exit_code = v
            .get("exit_code")
            .and_then(|x| x.as_i64())
            .map(|n| n as i32);
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
    };

    let event_id = commit_event(&repo, &store, &event, session)?;

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
