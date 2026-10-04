use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use colored::Colorize;
use serde_json::{Value, json};
use std::io::Read;

use crate::capture::{
    Exchange, PromptRecord, append_jsonl, claim_exchange, count_contained, last_prompt,
    load_unclaimed_exchanges_since, now_ms, overlap_is_significant, prompts_path,
    significant_lines,
};
use crate::cli::{HookArgs, HookEventArgs};
use crate::object::{Event, Snapshot, TreeEntry};
use crate::repo::Repo;
use crate::snapshot::{flatten_tree, snapshot_workspace};
use crate::store::Store;

mod cursor;

/// `re hook <agent>` — native capture where hooks exist.
///
/// Claude Code exposes lifecycle hooks (UserPromptSubmit, PostToolUse) that
/// hand us the *real* prompt and the *real* tool call — no inference needed.
/// This command wires them up in the project's `.claude/settings.json`:
///
/// - UserPromptSubmit → `re hook-event user-prompt` (stores the prompt)
/// - PostToolUse (Edit|Write|MultiEdit|NotebookEdit) → `re hook-event post-tool`
///   (records a full Causari event: snapshot, prompt, tool, file)
///
/// Claude Code's hooks carry no model, token or cost information. When it
/// also runs through `re proxy` (`ANTHROPIC_BASE_URL`), the post-tool event
/// borrows those from the one recent Claude exchange whose completion
/// contains the lines it just wrote, and claims that exchange.
///
/// Cursor has its own hooks (`.cursor/hooks.json`), wired by `re hook
/// cursor`; see [`cursor`]. Where hooks don't exist (custom agents),
/// `re proxy` + `re watch` cover the same ground via content correlation.
pub fn run(args: HookArgs) -> Result<()> {
    match args.target.as_str() {
        "claude-code" => {
            if args.user {
                return Err(anyhow!(
                    "--user is not supported for claude-code (hooks go to the project's .claude/settings.json)"
                ));
            }
            install_claude_code(args.dry_run)
        }
        "cursor" => cursor::install(args.user, args.dry_run),
        other => Err(anyhow!(
            "unknown hook target '{}' (supported: claude-code, cursor)",
            other
        )),
    }
}

const PROMPT_HOOK_CMD: &str = "re hook-event user-prompt";
const PRE_TOOL_HOOK_CMD: &str = "re hook-event pre-tool";
const TOOL_HOOK_CMD: &str = "re hook-event post-tool";
const SESSION_HOOK_CMD: &str = "re hook-event session-start";
const TOOL_MATCHER: &str = "Edit|Write|MultiEdit|NotebookEdit";
/// Max entries per section injected at session start — keep the context lean.
const SESSION_BRIEF_LIMIT: usize = 3;

fn install_claude_code(dry_run: bool) -> Result<()> {
    let repo = Repo::discover()?;
    let dir = repo.root.join(".claude");
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
    ensure_hook(hooks, "PreToolUse", Some(TOOL_MATCHER), PRE_TOOL_HOOK_CMD)?;
    ensure_hook(hooks, "PostToolUse", Some(TOOL_MATCHER), TOOL_HOOK_CMD)?;
    ensure_hook(hooks, "SessionStart", None, SESSION_HOOK_CMD)?;

    if dry_run {
        println!("{}", serde_json::to_string_pretty(&root)?);
        return Ok(());
    }
    std::fs::create_dir_all(&dir)?;
    std::fs::write(&path, serde_json::to_string_pretty(&root)?)?;

    println!(
        "{} Claude Code hooks installed in {}",
        "causari:".green().bold(),
        path.display().to_string().cyan()
    );
    println!("  UserPromptSubmit → captures every prompt");
    println!(
        "  PreToolUse ({}) → snapshots the tree the agent is about to change",
        TOOL_MATCHER
    );
    println!(
        "  PostToolUse ({}) → records the edit as a Causari event, diffed against that snapshot",
        TOOL_MATCHER
    );
    println!("  SessionStart → injects verified experience into every new session");
    println!();
    println!("  {}", crate::redact::STORAGE_NOTICE.bright_black());
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
    if hook_present(arr, command) {
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

/// Is `command` already wired somewhere in this list of hook entries?
/// Matched on the serialized entry, so any shape of entry (Claude Code's
/// nested `hooks[]`, Cursor's flat `{command}`) is recognised.
fn hook_present(entries: &[Value], command: &str) -> bool {
    entries.iter().any(|e| {
        serde_json::to_string(e)
            .unwrap_or_default()
            .contains(command)
    })
}

// ---------------------------------------------------------------------------
// `re hook-event` — the hidden command the hooks actually invoke
// ---------------------------------------------------------------------------

/// Invoked by the agent runtime with a JSON payload on stdin.
/// Must NEVER fail loudly: a non-zero exit or stderr noise would degrade the
/// agent session. Errors are swallowed by design.
///
/// `cursor:<event>` kinds are Cursor's native hooks; they always answer with
/// a JSON object on stdout, because Cursor reads one.
pub fn run_event(args: HookEventArgs) -> Result<()> {
    if let Some(event) = args.kind.strip_prefix("cursor:") {
        let mut input = String::new();
        let _ = std::io::stdin().read_to_string(&mut input);
        println!("{}", cursor::run_event(event, &input));
        return Ok(());
    }
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
            let mut record = PromptRecord {
                ts_ms: now_ms(),
                session_id,
                prompt,
                model: None,
                attachments: Vec::new(),
                redactions: 0,
            };
            record.redact_secrets();
            append_jsonl(&prompts_path(&repo), &record)
        }
        "pre-tool" => record_pre_state(&repo, session_id.as_deref()),
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

/// A pre-state captured by `PreToolUse`, waiting for its `PostToolUse`.
#[derive(serde::Serialize, serde::Deserialize)]
struct PendingPre {
    ts_ms: u64,
    #[serde(default)]
    session_id: Option<String>,
    snapshot_id: String,
}

fn pending_pre_path(repo: &Repo) -> std::path::PathBuf {
    repo.dir.join("capture").join("pending-pre.jsonl")
}

/// A pre-state older than this is stale: the tool call it belonged to never
/// produced a PostToolUse (denied, crashed, interrupted).
const PENDING_PRE_MAX_AGE_MS: u64 = 10 * 60 * 1000;

/// `PreToolUse`: snapshot the tree *before* the agent edits it.
///
/// This is what makes hook attribution exact. Without it the event's
/// pre-state is the previous event's post-state, and every change made in
/// between — a human edit, a checkout, a formatter — lands in the agent's
/// diff and gets that agent's prompt as its cause (review finding: "zero
/// false attribution" was false in the interleaved case).
fn record_pre_state(repo: &Repo, session_id: Option<&str>) -> Result<()> {
    let store = Store::new(repo);
    let _lock = repo.lock()?;
    let tree = snapshot_workspace(repo)?;
    let snapshot_id = store.write_snapshot(&Snapshot {
        tree,
        created_at: Utc::now().to_rfc3339(),
    })?;
    append_jsonl(
        &pending_pre_path(repo),
        &PendingPre {
            ts_ms: now_ms(),
            session_id: session_id.map(String::from),
            snapshot_id,
        },
    )
}

/// The most recent fresh pre-state for this session, if any; consumed on read.
fn take_pending_pre(repo: &Repo, session_id: Option<&str>) -> Option<String> {
    let path = pending_pre_path(repo);
    let raw = std::fs::read_to_string(&path).ok()?;
    let now = now_ms();
    let mut keep: Vec<PendingPre> = Vec::new();
    let mut found: Option<String> = None;
    for line in raw.lines().rev() {
        let Ok(p) = serde_json::from_str::<PendingPre>(line) else {
            continue;
        };
        if now.saturating_sub(p.ts_ms) > PENDING_PRE_MAX_AGE_MS {
            continue;
        }
        if found.is_none() && p.session_id.as_deref() == session_id {
            found = Some(p.snapshot_id);
            continue;
        }
        keep.push(p);
    }
    keep.reverse();
    let body: String = keep
        .iter()
        .filter_map(|p| serde_json::to_string(p).ok())
        .map(|l| l + "\n")
        .collect();
    let _ = crate::keys::write_atomic(&path, body.as_bytes());
    found
}

/// Drop every pending pre-state of a session: the agent loop ended, so no
/// PostToolUse of that session will ever claim them. Without this, the
/// snapshot of a denied or interrupted tool call would become the
/// pre-state of the next turn's first edit and absorb whatever a human
/// changed in between.
fn discard_pending_pre(repo: &Repo, session_id: Option<&str>) -> Result<()> {
    let path = pending_pre_path(repo);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    let body: String = raw
        .lines()
        .filter(|line| {
            serde_json::from_str::<PendingPre>(line)
                .map(|p| p.session_id.as_deref() != session_id)
                .unwrap_or(false)
        })
        .map(|l| l.to_string() + "\n")
        .collect();
    crate::keys::write_atomic(&path, body.as_bytes())
}

/// A file path as the ledger stores it: relative to the repository root,
/// forward slashes. `None` when the path lies outside the repository.
fn relative_to_repo(repo: &Repo, file: &str) -> Option<String> {
    let path = std::path::Path::new(file);
    let rel = match path.strip_prefix(&repo.root) {
        Ok(r) => r.to_path_buf(),
        // Symlinked roots (`/tmp` → `/private/tmp` on macOS) compare equal
        // only once both sides are canonical.
        Err(_) => {
            let root = std::fs::canonicalize(&repo.root).ok()?;
            let file = std::fs::canonicalize(path).ok()?;
            file.strip_prefix(&root).ok()?.to_path_buf()
        }
    };
    let rel = rel.to_string_lossy().replace('\\', "/");
    if rel.is_empty() { None } else { Some(rel) }
}

/// Record a full Causari event from a Claude Code PostToolUse payload.
fn record_tool_event(repo: &Repo, v: &Value, session_id: Option<&str>) -> Result<()> {
    let tool = v
        .get("tool_name")
        .and_then(|t| t.as_str())
        .unwrap_or("unknown")
        .to_string();
    let input = v.get("tool_input");
    let edits = input.map(edits_from_tool_input).unwrap_or_default();
    // Edit/Write/MultiEdit use `file_path`; NotebookEdit uses `notebook_path`.
    let file = input
        .and_then(|i| i.get("file_path").or_else(|| i.get("notebook_path")))
        .and_then(|f| f.as_str())
        .map(String::from);
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
    record_tool_action(
        repo,
        ToolAction {
            agent: "claude-code",
            evidence_source: "claude-code-hook",
            exchange_marker: "claude",
            session_id,
            tool,
            message,
            rel_file,
            model: None,
            reads: Vec::new(),
            added: None,
            edits,
        },
    )
    .map(|_| ())
}

/// `old_string`/`new_string` pairs a tool payload declared, in order.
/// Claude's Edit carries one pair; MultiEdit and Cursor carry `edits[]`.
fn edits_from_tool_input(input: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(pairs) = input.get("edits").and_then(Value::as_array) {
        for pair in pairs {
            if let (Some(old), Some(new)) = (
                pair.get("old_string").and_then(Value::as_str),
                pair.get("new_string").and_then(Value::as_str),
            ) {
                out.push((old.to_string(), new.to_string()));
            }
        }
        return out;
    }
    if let (Some(old), Some(new)) = (
        input.get("old_string").and_then(Value::as_str),
        input.get("new_string").and_then(Value::as_str),
    ) {
        out.push((old.to_string(), new.to_string()));
    }
    out
}

/// One tool call an agent runtime declared through a hook, in the terms
/// the ledger needs. Built from each runtime's own payload shape.
struct ToolAction<'a> {
    /// Agent id on the event (`claude-code`, `cursor`).
    agent: &'a str,
    /// `source` of the event's declared evidence.
    evidence_source: &'a str,
    /// Substring an exchange's User-Agent must carry to be this runtime's.
    exchange_marker: &'a str,
    /// The runtime's session or conversation id; scopes prompt and pre-state.
    session_id: Option<&'a str>,
    tool: String,
    message: String,
    /// The written file, relative to the repository root.
    rel_file: Option<String>,
    /// The model the runtime itself declared, when its payload carries one.
    model: Option<String>,
    /// Files the runtime declared as context for this action.
    reads: Vec<String>,
    /// The lines the runtime says it inserted. `None`: derive them from the
    /// snapshot diff of `rel_file`.
    added: Option<Vec<String>>,
    /// Declared replacements, oldest first. Used to rebuild a pre-state when
    /// no PreToolUse snapshot was taken.
    edits: Vec<(String, String)>,
}

/// The file as it was before `edits`, recovered by undoing them on `post`.
/// `None` when the payload declares nothing reversible: an empty new text
/// has no location to put the old text back, and a new text that is not in
/// the file was not this write.
fn reverse_edits(post: &str, edits: &[(String, String)]) -> Option<String> {
    if edits.is_empty() {
        return None;
    }
    let mut text = post.to_string();
    for (old, new) in edits.iter().rev() {
        if new.is_empty() {
            return None;
        }
        let at = text.rfind(new.as_str())?;
        text.replace_range(at..at + new.len(), old);
    }
    Some(text)
}

/// A pre-snapshot whose only difference from `post_tree` is `rel` restored
/// to the bytes before the declared edits. The human edit, the rename and
/// the formatter that happened since the previous event are already in
/// `post_tree`, so they stay on both sides and are not this agent's lines.
fn reversed_pre_snapshot(
    store: &Store,
    post_tree: &str,
    rel: Option<&str>,
    edits: &[(String, String)],
) -> Result<Option<String>> {
    let Some(rel) = rel else {
        return Ok(None);
    };
    let Some(blob) = crate::provenance::lookup_blob(store, post_tree, std::path::Path::new(rel))?
    else {
        return Ok(None);
    };
    let post = String::from_utf8_lossy(&store.read_blob(&blob)?).into_owned();
    let Some(pre_text) = reverse_edits(&post, edits) else {
        return Ok(None);
    };
    if pre_text == post {
        return Ok(None);
    }
    let pre_tree = graft_file(
        store,
        post_tree,
        rel,
        if pre_text.is_empty() {
            None
        } else {
            Some(pre_text.as_bytes())
        },
    )?;
    Ok(Some(store.write_snapshot(&Snapshot {
        tree: pre_tree,
        created_at: Utc::now().to_rfc3339(),
    })?))
}

/// `post_tree` with `rel` replaced by `contents`, or removed when `contents`
/// is `None` (the edit created the file). Intermediate directories that
/// become empty are removed with it.
fn graft_file(store: &Store, tree_id: &str, rel: &str, contents: Option<&[u8]>) -> Result<String> {
    let comps: Vec<&str> = rel.split('/').filter(|s| !s.is_empty()).collect();
    if comps.is_empty() {
        return Err(anyhow!("empty path"));
    }
    graft_at(store, tree_id, &comps, contents)
}

fn graft_at(
    store: &Store,
    tree_id: &str,
    comps: &[&str],
    contents: Option<&[u8]>,
) -> Result<String> {
    let mut tree = store.read_tree(tree_id)?;
    let name = comps[0];
    if comps.len() == 1 {
        match contents {
            Some(bytes) => {
                let id = store.write_blob(bytes)?;
                let exec = tree.entries.get(name).map(|e| e.exec).unwrap_or(false);
                tree.entries
                    .insert(name.to_string(), TreeEntry::blob(id, exec));
            }
            None => {
                tree.entries.remove(name);
            }
        }
        return store.write_tree(&tree);
    }
    let child = match tree.entries.get(name) {
        Some(entry) if entry.kind == "tree" => entry.id.clone(),
        _ => return Ok(tree_id.to_string()),
    };
    let new_child = graft_at(store, &child, &comps[1..], contents)?;
    let child_tree = store.read_tree(&new_child)?;
    if child_tree.entries.is_empty() {
        tree.entries.remove(name);
    } else {
        tree.entries
            .insert(name.to_string(), TreeEntry::tree(new_child));
    }
    store.write_tree(&tree)
}

/// Record a declared tool action as a full Causari event: pre-state,
/// post-state, prompt of the same session, the proxy exchange behind it
/// when exactly one qualifies. `Ok(None)` when nothing changed on disk.
fn record_tool_action(repo: &Repo, action: ToolAction) -> Result<Option<String>> {
    let store = Store::new(repo);
    let session_id = action.session_id;

    let _lock = repo.lock()?;
    let parent_id = crate::commit::resolve_parent(repo, None)?;
    let post_tree = snapshot_workspace(repo)?;
    // Prefer the pre-state captured by PreToolUse moments ago. Without it the
    // previous event's post-state is the whole gap since then — a human edit,
    // a rename, a formatter — and every inserted line would be this agent's.
    // A declared edit can be reversed onto the file as it sits now, which
    // puts that gap on both sides of the diff. Only a call that declares no
    // reversible edit falls back to the previous post-state.
    let pre_snapshot_id = match take_pending_pre(repo, session_id) {
        Some(id) => id,
        None => match reversed_pre_snapshot(
            &store,
            &post_tree,
            action.rel_file.as_deref(),
            &action.edits,
        )? {
            Some(id) => id,
            None => crate::commit::resolve_pre_snapshot(repo, &store, &parent_id)?,
        },
    };

    // Skip no-op tool calls (nothing actually changed on disk).
    if crate::commit::tree_unchanged(&store, &pre_snapshot_id, &post_tree)? {
        return Ok(None);
    }
    let post_snapshot_id = store.write_snapshot(&Snapshot {
        tree: post_tree,
        created_at: Utc::now().to_rfc3339(),
    })?;

    // The prompt must come from *this* session. Borrowing another session's
    // prompt would attribute an edit to a task it had nothing to do with;
    // when the runtime gave no session id, any prompt is the best we have.
    let prompt = match session_id {
        Some(_) => last_prompt(repo, session_id)?,
        None => last_prompt(repo, None)?,
    };
    let mut reads = action.reads;
    if reads.is_empty() {
        reads = prompt
            .as_ref()
            .map(|p| p.attachments.clone())
            .unwrap_or_default();
    }
    let declared_model = action
        .model
        .or_else(|| prompt.as_ref().and_then(|p| p.model.clone()));
    let prompt = prompt.map(|p| p.prompt);

    // The hook knows what was written; only the proxy knows which model
    // wrote it and what it cost. Merge the two when the evidence is
    // unambiguous, otherwise leave the event as declared.
    let exchange = action.rel_file.as_deref().and_then(|rel| {
        let since = now_ms().saturating_sub(HOOK_MERGE_WINDOW_MS);
        let exchanges = load_unclaimed_exchanges_since(repo, since).ok()?;
        if exchanges.is_empty() {
            return None;
        }
        let added = match action.added {
            Some(lines) => lines,
            None => inserted_lines_of(&store, &pre_snapshot_id, &post_snapshot_id, rel).ok()?,
        };
        matching_exchange(&exchanges, rel, &added, action.exchange_marker)
    });

    let event = Event {
        schema: "causari.event.v0.2".to_string(),
        parent: parent_id,
        agent: Some(action.agent.to_string()),
        model: declared_model.or_else(|| exchange.as_ref().and_then(|e| e.model.clone())),
        tool: Some(action.tool),
        message: Some(action.message),
        prompt,
        reasoning: None,
        reads,
        writes: action.rel_file.into_iter().collect(),
        tokens_in: exchange.as_ref().and_then(|e| e.tokens_in),
        tokens_out: exchange.as_ref().and_then(|e| e.tokens_out),
        cost_usd: exchange.as_ref().and_then(|e| e.cost_usd),
        pre_snapshot: pre_snapshot_id,
        post_snapshot: post_snapshot_id,
        exit_code: None,
        created_at: Utc::now().to_rfc3339(),
        evidence: Some(crate::object::Evidence::declared(action.evidence_source)),
        redactions: 0,
    };
    let id = crate::commit::commit_event(repo, &store, &event, None)?;
    if let Some(e) = &exchange {
        // Its tokens and dollars now belong to this event; `re watch` must
        // not attribute them a second time.
        claim_exchange(repo, e, &id)?;
    }
    Ok(Some(id))
}

/// How far back a proxy exchange may lie to be the completion behind a
/// hook event. Claude Code writes the file within seconds of the model's
/// answer; two minutes covers slow tool approval without reaching into
/// earlier turns.
const HOOK_MERGE_WINDOW_MS: u64 = 120 * 1000;

/// The proxy exchange behind this tool call, when exactly one qualifies.
///
/// Neither stream carries the other's key, so the join is by evidence: an
/// unclaimed exchange from this runtime's client (`agent_marker`, e.g.
/// `claude` — Claude Code's User-Agent starts with `claude-cli`) whose
/// completion contains the lines that just landed in the declared file —
/// the same overlap bar as `correlate`, or at least one significant line
/// together with the file's own name. Zero or several candidates →
/// `None`: a wrong model or cost is worse than none.
fn matching_exchange(
    exchanges: &[Exchange],
    rel_file: &str,
    added: &[String],
    agent_marker: &str,
) -> Option<Exchange> {
    let considered = significant_lines(added);
    if considered.is_empty() {
        return None;
    }
    let basename = std::path::Path::new(rel_file)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())?;
    let mut hits = exchanges.iter().filter(|e| {
        let from_runtime = e
            .agent
            .as_deref()
            .map(|a| a.to_ascii_lowercase().contains(agent_marker))
            .unwrap_or(false);
        if !from_runtime {
            return false;
        }
        let matched = count_contained(&considered, &e.response_text);
        overlap_is_significant(matched, considered.len())
            || (matched >= 1 && e.response_text.contains(&basename))
    });
    let only = hits.next()?;
    if hits.next().is_some() {
        return None;
    }
    Some(only.clone())
}

/// Lines inserted into one file between two snapshots. Scoped to the
/// declared path on purpose: whatever else changed in the tree is not
/// evidence about this tool call.
fn inserted_lines_of(
    store: &Store,
    pre_snapshot_id: &str,
    post_snapshot_id: &str,
    rel_file: &str,
) -> Result<Vec<String>> {
    use similar::{ChangeTag, TextDiff};
    let path = std::path::PathBuf::from(rel_file);
    let pre = flatten_tree(store, &store.read_snapshot(pre_snapshot_id)?.tree)?;
    let post = flatten_tree(store, &store.read_snapshot(post_snapshot_id)?.tree)?;
    let Some(post_id) = post.get(&path) else {
        return Ok(Vec::new());
    };
    let post_text = String::from_utf8(store.read_blob(post_id)?).unwrap_or_default();
    let pre_text = match pre.get(&path) {
        Some(id) if id == post_id => return Ok(Vec::new()),
        Some(id) => String::from_utf8(store.read_blob(id)?).unwrap_or_default(),
        None => String::new(),
    };
    Ok(TextDiff::from_lines(&pre_text, &post_text)
        .iter_all_changes()
        .filter(|c| c.tag() == ChangeTag::Insert)
        .map(|c| c.value().trim_end_matches('\n').to_string())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{exchanges_path, load_unclaimed_exchanges_since};

    const CODE: &str = "def refresh_token(user):\n    token = issue_token(user, scope=\"session\")\n    return rotate_every(token, hours=24)\n";

    fn exchange(id: &str, agent: &str, text: &str) -> Exchange {
        Exchange {
            id: Some(id.to_string()),
            ts_ms: now_ms(),
            agent: Some(agent.to_string()),
            model: Some("claude-sonnet-4-20250514".to_string()),
            prompt: Some("add token refresh".to_string()),
            response_text: text.to_string(),
            tokens_in: Some(900),
            tokens_out: Some(80),
            cost_usd: Some(0.0039),
            request_sha256: None,
            response_sha256: None,
            seal_id: None,
            truncated: false,
            redactions: 0,
        }
    }

    /// What the proxy stores for a Claude Code `Write`: the tool_use input's
    /// string leaves — the path and the file content.
    fn claude_write_completion(path: &str) -> String {
        format!("I'll add the helper.\n{path}\n{CODE}")
    }

    /// Simulate Claude Code's PreToolUse → file write → PostToolUse.
    fn run_hooks(repo: &Repo, session: &str, rel: &str, content: &str) {
        record_pre_state(repo, Some(session)).unwrap();
        let abs = repo.root.join(rel);
        std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
        std::fs::write(&abs, content).unwrap();
        let payload = json!({
            "session_id": session,
            "hook_event_name": "PostToolUse",
            "tool_name": "Write",
            "tool_input": {"file_path": abs.to_string_lossy(), "content": content},
            "tool_response": {"filePath": abs.to_string_lossy(), "success": true}
        });
        record_tool_event(repo, &payload, Some(session)).unwrap();
    }

    fn head_event(repo: &Repo) -> Event {
        let id = repo.head_event().unwrap().expect("an event was recorded");
        Store::new(repo).read_event(&id).unwrap()
    }

    #[test]
    fn hook_event_inherits_model_and_cost_from_the_one_matching_exchange() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        let abs = repo.root.join("src/auth.py");
        let ex = exchange(
            "ex-1",
            "claude-cli/1.0.83 (external, cli)",
            &claude_write_completion(&abs.to_string_lossy()),
        );
        append_jsonl(&exchanges_path(&repo), &ex).unwrap();
        // Noise the join must ignore: another client, unrelated content.
        append_jsonl(
            &exchanges_path(&repo),
            &exchange(
                "ex-2",
                "codex_cli_rs/0.40",
                &claude_write_completion("src/auth.py"),
            ),
        )
        .unwrap();
        append_jsonl(
            &exchanges_path(&repo),
            &exchange(
                "ex-3",
                "claude-cli/1.0.83",
                "Nothing to do with auth.py here.",
            ),
        )
        .unwrap();

        run_hooks(&repo, "sess-1", "src/auth.py", CODE);

        let ev = head_event(&repo);
        assert_eq!(ev.agent.as_deref(), Some("claude-code"));
        assert_eq!(ev.model.as_deref(), Some("claude-sonnet-4-20250514"));
        assert_eq!((ev.tokens_in, ev.tokens_out), (Some(900), Some(80)));
        assert_eq!(ev.cost_usd, Some(0.0039));
        assert_eq!(ev.writes, vec!["src/auth.py".to_string()]);
        // Still declared: the file and prompt come from the hook, not a guess.
        assert_eq!(
            ev.evidence,
            Some(crate::object::Evidence::declared("claude-code-hook"))
        );
        // The exchange is spent; `re watch` will not attribute it again.
        let left: Vec<String> = load_unclaimed_exchanges_since(&repo, 0)
            .unwrap()
            .into_iter()
            .filter_map(|e| e.id)
            .collect();
        assert_eq!(left, vec!["ex-2".to_string(), "ex-3".to_string()]);
    }

    #[test]
    fn ambiguous_or_absent_exchanges_leave_the_event_declared_only() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();

        // No exchange at all.
        run_hooks(&repo, "sess-1", "src/a.py", CODE);
        let ev = head_event(&repo);
        assert_eq!(ev.model, None);
        assert_eq!(ev.cost_usd, None);

        // Two Claude exchanges both containing the code (a retry): ambiguous.
        let text = claude_write_completion("src/b.py");
        append_jsonl(
            &exchanges_path(&repo),
            &exchange("ex-a", "claude-cli/1.0", &text),
        )
        .unwrap();
        append_jsonl(
            &exchanges_path(&repo),
            &exchange("ex-b", "claude-cli/1.0", &text),
        )
        .unwrap();
        run_hooks(&repo, "sess-1", "src/b.py", CODE);
        let ev = head_event(&repo);
        assert_eq!(ev.writes, vec!["src/b.py".to_string()]);
        assert_eq!(ev.model, None);
        assert_eq!(ev.tokens_in, None);
        assert_eq!(load_unclaimed_exchanges_since(&repo, 0).unwrap().len(), 2);
    }

    #[test]
    fn matching_requires_claude_agent_and_content_overlap() {
        let matching_exchange = |ex: &[Exchange], rel: &str, added: &[String]| {
            matching_exchange(ex, rel, added, "claude")
        };
        let added: Vec<String> = CODE.lines().map(String::from).collect();
        let good = exchange(
            "g",
            "claude-cli/1.0",
            &claude_write_completion("src/auth.py"),
        );
        assert_eq!(
            matching_exchange(std::slice::from_ref(&good), "src/auth.py", &added)
                .and_then(|e| e.id)
                .as_deref(),
            Some("g")
        );
        // Not from a Claude client.
        let other = exchange("o", "aider/0.86", &claude_write_completion("src/auth.py"));
        assert!(matching_exchange(&[other], "src/auth.py", &added).is_none());
        // From Claude, but the completion does not contain what was written.
        let unrelated = exchange("u", "claude-cli/1.0", "Sure, renaming the variable.");
        assert!(matching_exchange(&[unrelated], "src/auth.py", &added).is_none());
        // One line of many, but the file's own name is in the completion:
        // enough (a small Edit inside a big file).
        let one_line = vec![
            "    return rotate_every(token, hours=24)".to_string(),
            "x = 1".to_string(),
        ];
        let edit = exchange(
            "e",
            "claude-cli/1.0",
            "Editing auth.py:\n    return rotate_every(token, hours=24)",
        );
        assert!(matching_exchange(std::slice::from_ref(&edit), "src/auth.py", &one_line).is_some());
        // Same line, but a different file: the name does not match and one
        // line out of five is below the overlap bar.
        let many = vec![
            "    return rotate_every(token, hours=24)".to_string(),
            "alpha = compute_alpha(input)".to_string(),
            "beta = compute_beta(input)".to_string(),
            "gamma = compute_gamma(input)".to_string(),
            "delta = compute_delta(input)".to_string(),
        ];
        assert!(matching_exchange(&[edit], "src/other.py", &many).is_none());
        // Nothing significant was inserted: nothing to match on.
        assert!(matching_exchange(&[good], "src/auth.py", &["}".to_string()]).is_none());
    }
}
