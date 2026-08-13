use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use colored::Colorize;
use serde_json::{Value, json};
use std::io::Read;

use crate::capture::{PromptRecord, append_jsonl, last_prompt, now_ms, prompts_path};
use crate::cli::{HookArgs, HookEventArgs};
use crate::object::{Event, Snapshot};
use crate::repo::Repo;
use crate::snapshot::snapshot_workspace;
use crate::store::Store;

/// `re hook claude-code` — native capture where hooks exist.
///
/// Claude Code exposes lifecycle hooks (UserPromptSubmit, PostToolUse) that
/// hand us the *real* prompt and the *real* tool call — no inference needed.
/// This command wires them up in the project's `.claude/settings.json`:
///
/// - UserPromptSubmit → `re hook-event user-prompt` (stores the prompt)
/// - PostToolUse (Edit|Write|MultiEdit|NotebookEdit) → `re hook-event post-tool`
///   (records a full Causari event: snapshot, prompt, tool, file)
///
/// Where hooks don't exist (Cursor, custom agents), `re proxy` + `re watch`
/// cover the same ground via content correlation.
pub fn run(args: HookArgs) -> Result<()> {
    match args.target.as_str() {
        "claude-code" => install_claude_code(),
        other => Err(anyhow!(
            "unknown hook target '{}' (supported: claude-code)",
            other
        )),
    }
}

const PROMPT_HOOK_CMD: &str = "re hook-event user-prompt";
const TOOL_HOOK_CMD: &str = "re hook-event post-tool";
const SESSION_HOOK_CMD: &str = "re hook-event session-start";
const TOOL_MATCHER: &str = "Edit|Write|MultiEdit|NotebookEdit";
/// Max entries per section injected at session start — keep the context lean.
const SESSION_BRIEF_LIMIT: usize = 3;

fn install_claude_code() -> Result<()> {
    let repo = Repo::discover()?;
    let dir = repo.root.join(".claude");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("settings.json");

    let mut root: Value = if path.exists() {
        let raw = std::fs::read_to_string(&path)?;
        serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?
    } else {
        json!({})
    };

    let hooks = root
        .as_object_mut()
        .ok_or_else(|| anyhow!("settings.json root is not an object"))?
        .entry("hooks")
        .or_insert_with(|| json!({}));

    ensure_hook(hooks, "UserPromptSubmit", None, PROMPT_HOOK_CMD)?;
    ensure_hook(hooks, "PostToolUse", Some(TOOL_MATCHER), TOOL_HOOK_CMD)?;
    ensure_hook(hooks, "SessionStart", None, SESSION_HOOK_CMD)?;

    std::fs::write(&path, serde_json::to_string_pretty(&root)?)?;

    println!(
        "{} Claude Code hooks installed in {}",
        "causari:".green().bold(),
        path.display().to_string().cyan()
    );
    println!("  UserPromptSubmit → captures every prompt");
    println!(
        "  PostToolUse ({}) → records every edit as a Causari event",
        TOOL_MATCHER
    );
    println!("  SessionStart → injects verified experience into every new session");
    println!();
    println!(
        "  {} restart Claude Code (or run /hooks) to load them.",
        "note:".yellow()
    );
    Ok(())
}

/// Idempotently add our hook entry for `kind` unless already present.
fn ensure_hook(hooks: &mut Value, kind: &str, matcher: Option<&str>, command: &str) -> Result<()> {
    let entries = hooks
        .as_object_mut()
        .ok_or_else(|| anyhow!("'hooks' is not an object"))?
        .entry(kind)
        .or_insert_with(|| json!([]));
    let arr = entries
        .as_array_mut()
        .ok_or_else(|| anyhow!("'hooks.{}' is not an array", kind))?;
    let already = arr.iter().any(|e| {
        serde_json::to_string(e)
            .unwrap_or_default()
            .contains(command)
    });
    if already {
        return Ok(());
    }
    let mut entry = json!({
        "hooks": [{ "type": "command", "command": command }]
    });
    if let Some(m) = matcher {
        entry["matcher"] = json!(m);
    }
    arr.push(entry);
    Ok(())
}

// ---------------------------------------------------------------------------
// `re hook-event` — the hidden command the hooks actually invoke
// ---------------------------------------------------------------------------

/// Invoked by the agent runtime with a JSON payload on stdin.
/// Must NEVER fail loudly: a non-zero exit or stderr noise would degrade the
/// agent session. Errors are swallowed by design.
pub fn run_event(args: HookEventArgs) -> Result<()> {
    let _ = run_event_inner(&args.kind);
    Ok(())
}

fn run_event_inner(kind: &str) -> Result<()> {
    let repo = Repo::discover()?;
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let v: Value = serde_json::from_str(&input)?;
    let session_id = v
        .get("session_id")
        .and_then(|s| s.as_str())
        .map(String::from);

    match kind {
        "user-prompt" => {
            let prompt = v
                .get("prompt")
                .and_then(|p| p.as_str())
                .unwrap_or_default()
                .to_string();
            if prompt.is_empty() {
                return Ok(());
            }
            append_jsonl(
                &prompts_path(&repo),
                &PromptRecord {
                    ts_ms: now_ms(),
                    session_id,
                    prompt,
                },
            )
        }
        "post-tool" => record_tool_event(&repo, &v, session_id.as_deref()),
        // SessionStart: whatever we print on stdout is added to the agent's
        // context. Inject the trust-ranked experience briefing so every new
        // session — regardless of which model is behind it — starts with the
        // lessons this repository has already paid for. Silent when there is
        // no experience yet: zero noise on fresh repos. Never bumps recall
        // counters (trust is earned by explicit use, not by injection).
        "session-start" => {
            if let Some(md) =
                crate::commands::brief::render(&repo, &[], SESSION_BRIEF_LIMIT, false)?
            {
                print!("{md}");
            }
            Ok(())
        }
        other => Err(anyhow!("unknown hook-event kind '{}'", other)),
    }
}

/// Record a full Causari event from a Claude Code PostToolUse payload.
fn record_tool_event(repo: &Repo, v: &Value, session_id: Option<&str>) -> Result<()> {
    let store = Store::new(repo);
    let tool = v
        .get("tool_name")
        .and_then(|t| t.as_str())
        .unwrap_or("unknown")
        .to_string();
    let file = v
        .get("tool_input")
        .and_then(|i| i.get("file_path"))
        .and_then(|f| f.as_str())
        .map(String::from);

    let _lock = repo.lock()?;
    let parent_id = crate::commit::resolve_parent(repo, None)?;
    let pre_snapshot_id = crate::commit::resolve_pre_snapshot(repo, &store, &parent_id)?;
    let post_tree = snapshot_workspace(repo)?;

    // Skip no-op tool calls (nothing actually changed on disk).
    if let Some(pid) = &parent_id {
        let pre_tree = store
            .read_snapshot(&store.read_event(pid)?.post_snapshot)?
            .tree;
        if pre_tree == post_tree {
            return Ok(());
        }
    }
    let post_snapshot_id = store.write_snapshot(&Snapshot {
        tree: post_tree,
        created_at: Utc::now().to_rfc3339(),
    })?;

    let prompt = last_prompt(repo, session_id)?
        .or_else(|| last_prompt(repo, None).ok().flatten())
        .map(|p| p.prompt);

    let rel_file = file.as_deref().map(|f| {
        std::path::Path::new(f)
            .strip_prefix(&repo.root)
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| f.replace('\\', "/"))
    });
    let message = match &rel_file {
        Some(f) => format!("{} {}", tool, f),
        None => tool.clone(),
    };

    let event = Event {
        schema: "causari.event.v0.2".to_string(),
        parent: parent_id,
        agent: Some("claude-code".to_string()),
        model: None,
        tool: Some(tool),
        message: Some(message),
        prompt,
        reasoning: None,
        reads: Vec::new(),
        writes: rel_file.into_iter().collect(),
        tokens_in: None,
        tokens_out: None,
        cost_usd: None,
        pre_snapshot: pre_snapshot_id,
        post_snapshot: post_snapshot_id,
        exit_code: None,
        created_at: Utc::now().to_rfc3339(),
    };
    crate::commit::commit_event(repo, &store, &event, None)?;
    Ok(())
}
