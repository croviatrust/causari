//! `re hook cursor` — Cursor's native hooks (`hooks.json`).
//!
//! Cursor spawns each hook as a process with a JSON payload on stdin and
//! reads a JSON object back on stdout. Every payload carries the
//! conversation id and the model, so the ledger gets the prompt, the edit
//! and the model from the runtime itself: exact bytes, no inference.
//!
//! Installed hooks (all call `re hook-event cursor:<event>`):
//!
//! - `beforeSubmitPrompt` → the prompt, its attachments and the model,
//!   keyed by `conversation_id`
//! - `preToolUse` (Shell|Write) → snapshot of the tree the tool is about
//!   to change, so the event's diff is exactly that tool call
//! - `afterFileEdit` → a Causari event for the written file, joined to
//!   the conversation's prompt
//! - `afterShellExecution` → a Causari event when PreToolUse captured the
//!   tree, the command as its message. Without that snapshot the command
//!   is not given every change since the previous event.
//! - `afterAgentResponse` → the agent's answer, stored as an exchange next
//!   to its prompt
//! - `stop` → pre-states the turn never used are dropped
//! - `sessionStart` → the experience briefing as `additional_context`
//!
//! `preToolUse` is a permission hook: an answer that is not valid JSON
//! blocks the tool. This module therefore always prints one, whatever
//! happened inside.

use anyhow::{Context, Result, anyhow};
use colored::Colorize;
use serde_json::{Value, json};
use std::path::PathBuf;

use super::{
    SESSION_BRIEF_LIMIT, ToolAction, discard_pending_pre, hook_present, record_pre_state,
    record_tool_action, relative_to_repo,
};
use crate::capture::{
    Exchange, PromptRecord, append_jsonl, exchanges_path, last_prompt, new_exchange_id, now_ms,
    prompts_path,
};
use crate::repo::Repo;

const AGENT: &str = "cursor";
const EVIDENCE_SOURCE: &str = "cursor-hook";
/// Matcher of `preToolUse`: the two tools whose effect on the tree we
/// record afterwards (`afterFileEdit`, `afterShellExecution`).
const PRE_TOOL_MATCHER: &str = "Shell|Write";

/// The events `re hook cursor` wires, in the order they are written.
const EVENTS: &[(&str, Option<&str>)] = &[
    ("sessionStart", None),
    ("beforeSubmitPrompt", None),
    ("preToolUse", Some(PRE_TOOL_MATCHER)),
    ("afterFileEdit", None),
    ("afterShellExecution", None),
    ("afterAgentResponse", None),
    ("stop", None),
];

fn command_for(event: &str) -> String {
    format!("re hook-event cursor:{event}")
}

// ---------------------------------------------------------------------------
// Install
// ---------------------------------------------------------------------------

pub(super) fn install(user: bool, dry_run: bool) -> Result<()> {
    let path = if user {
        home_dir()?.join(".cursor").join("hooks.json")
    } else {
        Repo::discover()?.root.join(".cursor").join("hooks.json")
    };

    let mut root: Value = if path.exists() {
        let raw = std::fs::read_to_string(&path)?;
        serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?
    } else {
        json!({})
    };
    merge_hooks(&mut root)?;
    let text = format!("{}\n", serde_json::to_string_pretty(&root)?);

    if dry_run {
        print!("{text}");
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;

    println!(
        "{} Cursor hooks installed in {}",
        "causari:".green().bold(),
        path.display().to_string().cyan()
    );
    println!("  beforeSubmitPrompt → captures every prompt, with the model behind it");
    println!(
        "  preToolUse ({}) → snapshots the tree the agent is about to change",
        PRE_TOOL_MATCHER
    );
    println!("  afterFileEdit → records the edit as a Causari event, diffed against that snapshot");
    println!("  afterShellExecution → records a command when preToolUse captured the tree");
    println!("  afterAgentResponse → keeps the agent's answer next to its prompt");
    println!("  stop → drops the pre-states the turn never used");
    println!("  sessionStart → injects a briefing of past skills (recall counts, not proofs)");
    println!();
    println!("  {}", crate::redact::STORAGE_NOTICE.bright_black());
    println!();
    println!(
        "  {} `re` must be on PATH for Cursor to run them. Cursor reloads hooks.json on save.",
        "note:".yellow()
    );
    if user {
        println!(
            "  {} user hooks run on this machine only; cloud agents read the project's .cursor/hooks.json.",
            "note:".yellow()
        );
    }
    Ok(())
}

/// `~`: `HOME` on Unix, `USERPROFILE` on Windows.
fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("cannot locate the home directory (HOME / USERPROFILE unset)"))
}

/// Idempotently add every Causari hook to a `hooks.json` document, leaving
/// whatever else is in it untouched. `version` is set to 1 only when
/// absent.
fn merge_hooks(root: &mut Value) -> Result<()> {
    let obj = root
        .as_object_mut()
        .ok_or_else(|| anyhow!("hooks.json root is not an object"))?;
    obj.entry("version").or_insert_with(|| json!(1));
    let hooks = obj
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| anyhow!("'hooks' is not an object"))?;
    for (event, matcher) in EVENTS {
        let arr = hooks
            .entry(*event)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(|| anyhow!("'hooks.{}' is not an array", event))?;
        let command = command_for(event);
        if hook_present(arr, &command) {
            continue;
        }
        let mut entry = json!({ "command": command });
        if let Some(m) = matcher {
            entry["matcher"] = json!(m);
        }
        arr.push(entry);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Handle one Cursor hook payload and return the JSON Cursor expects on
/// stdout. Never fails: on any error the event's neutral answer is
/// returned, so a missing ledger or a malformed payload can neither block
/// a tool nor a prompt.
pub(super) fn run_event(event: &str, input: &str) -> Value {
    handle(event, input).unwrap_or_else(|_| neutral_response(event))
}

/// The answer that changes nothing: `allow` for the permission hook,
/// `continue` for the prompt gate, `{}` everywhere else.
fn neutral_response(event: &str) -> Value {
    match event {
        "preToolUse" => json!({ "permission": "allow" }),
        "beforeSubmitPrompt" => json!({ "continue": true }),
        _ => json!({}),
    }
}

fn handle(event: &str, input: &str) -> Result<Value> {
    let v: Value = serde_json::from_str(input)?;
    let repo = discover_repo(&v)?;
    handle_in(&repo, event, &v)
}

/// The ledger this hook reports to. `CURSOR_PROJECT_DIR` and
/// `workspace_roots` come first: user-level hooks run from `~/.cursor/`,
/// where the working directory says nothing about the project.
fn discover_repo(v: &Value) -> Result<Repo> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dir) = std::env::var_os("CURSOR_PROJECT_DIR").filter(|d| !d.is_empty()) {
        candidates.push(PathBuf::from(dir));
    }
    for root in v
        .get("workspace_roots")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        candidates.push(PathBuf::from(root));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd);
    }
    candidates
        .iter()
        .find_map(|c| Repo::discover_from(c).ok())
        .ok_or_else(|| anyhow!("no causari repository for this Cursor workspace"))
}

fn handle_in(repo: &Repo, event: &str, v: &Value) -> Result<Value> {
    let common = Common::parse(v);
    let session = common.conversation_id.as_deref();
    match event {
        // Whatever we return as `additional_context` joins the conversation's
        // initial context: the trust-ranked experience briefing, silent on
        // repositories without experience yet (same as Claude Code's
        // SessionStart). A briefing does not write the legacy recall count.
        "sessionStart" => match crate::commands::brief::render(repo, &[], SESSION_BRIEF_LIMIT)? {
            Some(md) => Ok(json!({ "additional_context": md })),
            None => Ok(json!({})),
        },
        "beforeSubmitPrompt" => {
            if let Some(p) = PromptSubmit::parse(v) {
                let attachments = p
                    .attachments
                    .iter()
                    .filter_map(|a| relative_to_repo(repo, a))
                    .collect();
                let mut record = PromptRecord {
                    ts_ms: now_ms(),
                    session_id: common.conversation_id.clone(),
                    prompt: p.prompt,
                    model: common.model.clone(),
                    attachments,
                    redactions: 0,
                };
                record.redact_secrets();
                append_jsonl(&prompts_path(repo), &record)?;
            }
            Ok(json!({ "continue": true }))
        }
        "preToolUse" => {
            record_pre_state(repo, session)?;
            Ok(json!({ "permission": "allow" }))
        }
        "afterFileEdit" => {
            let Some(edit) = FileEdit::parse(v) else {
                return Ok(json!({}));
            };
            // A file outside the repository is not this ledger's business.
            let Some(rel) = relative_to_repo(repo, &edit.file_path) else {
                return Ok(json!({}));
            };
            record_tool_action(
                repo,
                ToolAction {
                    agent: AGENT,
                    evidence_source: EVIDENCE_SOURCE,
                    exchange_marker: AGENT,
                    session_id: session,
                    tool: "Write".to_string(),
                    message: format!("Write {rel}"),
                    rel_file: Some(rel),
                    model: common.model.clone(),
                    reads: Vec::new(),
                    added: Some(edit.added),
                    edits: edit.edits,
                },
            )?;
            Ok(json!({}))
        }
        "afterShellExecution" => {
            let Some(run) = ShellRun::parse(v) else {
                return Ok(json!({}));
            };
            record_tool_action(
                repo,
                ToolAction {
                    agent: AGENT,
                    evidence_source: EVIDENCE_SOURCE,
                    exchange_marker: AGENT,
                    session_id: session,
                    tool: "Shell".to_string(),
                    message: run.command,
                    rel_file: None,
                    model: common.model.clone(),
                    reads: Vec::new(),
                    added: None,
                    edits: Vec::new(),
                },
            )?;
            Ok(json!({}))
        }
        // The answer completes the exchange the prompt opened. Stored where
        // the proxy stores completions, under agent `cursor`, so the same
        // content join (`matching_exchange`, `re watch`) can find it; it
        // carries no tokens or cost, Cursor does not report them.
        "afterAgentResponse" => {
            let Some(text) = response_text(v) else {
                return Ok(json!({}));
            };
            let prompt = last_prompt(repo, session)?;
            let mut exchange = Exchange {
                id: Some(new_exchange_id()?),
                ts_ms: now_ms(),
                agent: Some(AGENT.to_string()),
                model: common
                    .model
                    .clone()
                    .or_else(|| prompt.as_ref().and_then(|p| p.model.clone())),
                prompt: prompt.map(|p| p.prompt),
                response_text: text,
                tokens_in: None,
                tokens_out: None,
                cost_usd: None,
                request_sha256: None,
                response_sha256: None,
                seal_id: None,
                truncated: false,
                redactions: 0,
            };
            exchange.redact_secrets();
            append_jsonl(&exchanges_path(repo), &exchange)?;
            Ok(json!({}))
        }
        "stop" => {
            discard_pending_pre(repo, session)?;
            Ok(json!({}))
        }
        other => Err(anyhow!("unknown cursor hook event '{}'", other)),
    }
}

// ---------------------------------------------------------------------------
// Payloads (https://cursor.com/docs/agent/hooks, "Reference")
// ---------------------------------------------------------------------------

/// Fields every Cursor hook payload carries.
#[derive(Debug, Default, PartialEq)]
struct Common {
    /// Stable across the turns of one conversation; the ledger's session key.
    conversation_id: Option<String>,
    /// `model_id` (structured, e.g. `claude-opus-4-7`) when present,
    /// otherwise the legacy `model` slug (`claude-opus-4-7-thinking-max`).
    model: Option<String>,
}

impl Common {
    fn parse(v: &Value) -> Self {
        Self {
            conversation_id: str_field(v, "conversation_id"),
            model: str_field(v, "model_id").or_else(|| str_field(v, "model")),
        }
    }
}

/// `beforeSubmitPrompt`: `prompt`, `attachments[].file_path`.
#[derive(Debug, PartialEq)]
struct PromptSubmit {
    prompt: String,
    /// Absolute paths of attached files and rules.
    attachments: Vec<String>,
}

impl PromptSubmit {
    fn parse(v: &Value) -> Option<Self> {
        let prompt = str_field(v, "prompt")?;
        let attachments = v
            .get("attachments")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|a| str_field(a, "file_path"))
            .collect();
        Some(Self {
            prompt,
            attachments,
        })
    }
}

/// `afterFileEdit`: `file_path` (absolute), `edits[].{old_string,new_string}`.
#[derive(Debug, PartialEq)]
struct FileEdit {
    file_path: String,
    /// Every line of every `new_string`, verbatim: what the agent inserted.
    added: Vec<String>,
    /// Declared replacements, in order. Reversed when no pre-state was captured.
    edits: Vec<(String, String)>,
}

impl FileEdit {
    fn parse(v: &Value) -> Option<Self> {
        let file_path = str_field(v, "file_path")?;
        let pairs = v.get("edits").and_then(Value::as_array);
        let edits = pairs
            .into_iter()
            .flatten()
            .filter_map(|e| {
                Some((
                    e.get("old_string").and_then(Value::as_str)?.to_string(),
                    e.get("new_string").and_then(Value::as_str)?.to_string(),
                ))
            })
            .collect::<Vec<_>>();
        let added = edits
            .iter()
            .flat_map(|(_, new)| new.lines().map(String::from).collect::<Vec<_>>())
            .collect();
        Some(Self {
            file_path,
            added,
            edits,
        })
    }
}

/// `afterShellExecution`: `command`, `output`, `duration`, `sandbox`.
#[derive(Debug, PartialEq)]
struct ShellRun {
    command: String,
}

impl ShellRun {
    fn parse(v: &Value) -> Option<Self> {
        Some(Self {
            command: str_field(v, "command")?,
        })
    }
}

/// `afterAgentResponse`: `text`.
fn response_text(v: &Value) -> Option<String> {
    str_field(v, "text")
}

/// A non-empty string field.
fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::load_unclaimed_exchanges_since;
    use crate::object::{Event, Evidence};
    use crate::provenance::{chain_to, find_line_origin, line_owners};
    use crate::store::Store;

    fn expected_fresh() -> Value {
        json!({
            "version": 1,
            "hooks": {
                "sessionStart": [{ "command": "re hook-event cursor:sessionStart" }],
                "beforeSubmitPrompt": [{ "command": "re hook-event cursor:beforeSubmitPrompt" }],
                "preToolUse": [{ "command": "re hook-event cursor:preToolUse", "matcher": "Shell|Write" }],
                "afterFileEdit": [{ "command": "re hook-event cursor:afterFileEdit" }],
                "afterShellExecution": [{ "command": "re hook-event cursor:afterShellExecution" }],
                "afterAgentResponse": [{ "command": "re hook-event cursor:afterAgentResponse" }],
                "stop": [{ "command": "re hook-event cursor:stop" }]
            }
        })
    }

    #[test]
    fn merge_into_empty_document_writes_the_full_hooks_json() {
        let mut root = json!({});
        merge_hooks(&mut root).unwrap();
        assert_eq!(root, expected_fresh());
    }

    #[test]
    fn merge_keeps_other_peoples_hooks_and_their_version() {
        let mut root = json!({
            "version": 1,
            "hooks": {
                "afterFileEdit": [{ "command": ".cursor/hooks/format.sh" }],
                "beforeShellExecution": [
                    { "command": "./hooks/approve-network.sh", "timeout": 30, "matcher": "curl|wget|nc" }
                ],
                "stop": [{ "command": "bun run .cursor/hooks/track-stop.ts --stop", "loop_limit": 10 }]
            }
        });
        merge_hooks(&mut root).unwrap();
        assert_eq!(root["version"], json!(1));
        // Theirs first, ours appended.
        assert_eq!(
            root["hooks"]["afterFileEdit"],
            json!([
                { "command": ".cursor/hooks/format.sh" },
                { "command": "re hook-event cursor:afterFileEdit" }
            ])
        );
        assert_eq!(
            root["hooks"]["beforeShellExecution"],
            json!([{ "command": "./hooks/approve-network.sh", "timeout": 30, "matcher": "curl|wget|nc" }])
        );
        assert_eq!(
            root["hooks"]["stop"],
            json!([
                { "command": "bun run .cursor/hooks/track-stop.ts --stop", "loop_limit": 10 },
                { "command": "re hook-event cursor:stop" }
            ])
        );
        assert_eq!(
            root["hooks"]["preToolUse"],
            json!([{ "command": "re hook-event cursor:preToolUse", "matcher": "Shell|Write" }])
        );
    }

    #[test]
    fn merge_is_idempotent() {
        let mut root = json!({});
        merge_hooks(&mut root).unwrap();
        let once = root.clone();
        merge_hooks(&mut root).unwrap();
        assert_eq!(root, once);
        // Also when a user hand-wrote our command with extra options.
        let mut custom = json!({
            "version": 1,
            "hooks": { "afterFileEdit": [{ "command": "re hook-event cursor:afterFileEdit", "timeout": 60 }] }
        });
        merge_hooks(&mut custom).unwrap();
        assert_eq!(
            custom["hooks"]["afterFileEdit"],
            json!([{ "command": "re hook-event cursor:afterFileEdit", "timeout": 60 }])
        );
    }

    #[test]
    fn merge_refuses_a_non_object_document() {
        assert!(merge_hooks(&mut json!([])).is_err());
        assert!(merge_hooks(&mut json!({ "hooks": "nope" })).is_err());
    }

    // -- Payloads, shaped as the reference documents them.

    fn common_fields(conversation: &str) -> Value {
        json!({
            "conversation_id": conversation,
            "generation_id": "gen-7f3a",
            "model": "claude-opus-4-7-thinking-max",
            "model_id": "claude-opus-4-7",
            "model_params": [{ "id": "thinking", "value": "true" }],
            "hook_event_name": "afterFileEdit",
            "cursor_version": "1.7.2",
            "workspace_roots": ["/home/dev/project"],
            "user_email": null,
            "transcript_path": null
        })
    }

    fn with(mut base: Value, extra: Value) -> Value {
        base.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        base
    }

    #[test]
    fn common_prefers_model_id_over_the_legacy_slug() {
        let c = Common::parse(&common_fields("conv-1"));
        assert_eq!(
            c,
            Common {
                conversation_id: Some("conv-1".into()),
                model: Some("claude-opus-4-7".into()),
            }
        );
        let legacy = json!({ "conversation_id": "conv-2", "model": "gpt-5" });
        assert_eq!(Common::parse(&legacy).model.as_deref(), Some("gpt-5"));
        assert_eq!(Common::parse(&json!({})), Common::default());
    }

    #[test]
    fn parse_before_submit_prompt() {
        let v = with(
            common_fields("conv-1"),
            json!({
                "prompt": "Add a health-check endpoint\nreturning build sha and uptime",
                "attachments": [
                    { "type": "file", "file_path": "/home/dev/project/spec/health.md" },
                    { "type": "rule", "file_path": "/home/dev/project/.cursor/rules/api.mdc" }
                ]
            }),
        );
        assert_eq!(
            PromptSubmit::parse(&v).unwrap(),
            PromptSubmit {
                prompt: "Add a health-check endpoint\nreturning build sha and uptime".into(),
                attachments: vec![
                    "/home/dev/project/spec/health.md".into(),
                    "/home/dev/project/.cursor/rules/api.mdc".into()
                ],
            }
        );
        assert!(PromptSubmit::parse(&with(common_fields("c"), json!({ "prompt": "" }))).is_none());
    }

    #[test]
    fn parse_after_file_edit_keeps_every_inserted_line_verbatim() {
        let v = with(
            common_fields("conv-1"),
            json!({
                "file_path": "/home/dev/project/src/health.ts",
                "edits": [
                    { "old_string": "", "new_string": "export function health() {\n  return { sha: SHA, uptime: process.uptime() };\n}\n" },
                    { "old_string": "// TODO", "new_string": "  // wired in app.ts" }
                ]
            }),
        );
        assert_eq!(
            FileEdit::parse(&v).unwrap(),
            FileEdit {
                file_path: "/home/dev/project/src/health.ts".into(),
                added: vec![
                    "export function health() {".into(),
                    "  return { sha: SHA, uptime: process.uptime() };".into(),
                    "}".into(),
                    "  // wired in app.ts".into(),
                ],
                edits: vec![
                    (
                        "".into(),
                        "export function health() {\n  return { sha: SHA, uptime: process.uptime() };\n}\n".into(),
                    ),
                    ("// TODO".into(), "  // wired in app.ts".into()),
                ],
            }
        );
        assert!(FileEdit::parse(&common_fields("c")).is_none());
    }

    #[test]
    fn parse_shell_response_and_neutral_answers() {
        let shell = with(
            common_fields("conv-1"),
            json!({ "command": "npm test", "output": "All tests passed\n", "duration": 1234, "sandbox": false }),
        );
        assert_eq!(
            ShellRun::parse(&shell).unwrap(),
            ShellRun {
                command: "npm test".into()
            }
        );
        let resp = with(
            common_fields("conv-1"),
            json!({ "text": "Done. The endpoint is live." }),
        );
        assert_eq!(
            response_text(&resp).as_deref(),
            Some("Done. The endpoint is live.")
        );
        assert_eq!(
            neutral_response("preToolUse"),
            json!({ "permission": "allow" })
        );
        assert_eq!(
            neutral_response("beforeSubmitPrompt"),
            json!({ "continue": true })
        );
        assert_eq!(neutral_response("afterFileEdit"), json!({}));
        // Garbage in, neutral answer out — and never an error.
        assert_eq!(
            run_event("preToolUse", "not json"),
            json!({ "permission": "allow" })
        );
        assert_eq!(run_event("stop", ""), json!({}));
    }

    // -- End to end inside one repository.

    fn head_event(repo: &Repo) -> Event {
        let id = repo.head_event().unwrap().expect("an event was recorded");
        Store::new(repo).read_event(&id).unwrap()
    }

    const CODE: &str =
        "export function health() {\n  return { sha: SHA, uptime: process.uptime() };\n}\n";

    /// Cursor's order: `preToolUse` fires, the tool writes, `afterFileEdit`
    /// fires. Returns the `afterFileEdit` payload.
    fn pre_tool_then_write(repo: &Repo, conversation: &str, rel: &str, new_string: &str) -> Value {
        let pre = with(common_fields(conversation), json!({ "tool_name": "Write" }));
        assert_eq!(
            handle_in(repo, "preToolUse", &pre).unwrap(),
            json!({ "permission": "allow" })
        );
        let abs = repo.root.join(rel);
        std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
        std::fs::write(&abs, new_string).unwrap();
        with(
            common_fields(conversation),
            json!({
                "file_path": abs.to_string_lossy(),
                "edits": [{ "old_string": "", "new_string": new_string }]
            }),
        )
    }

    fn pre_tool_shell(repo: &Repo, conversation: &str) {
        let pre = with(common_fields(conversation), json!({ "tool_name": "Shell" }));
        handle_in(repo, "preToolUse", &pre).unwrap();
    }

    #[test]
    fn prompt_then_edit_records_a_cursor_event_with_prompt_model_and_reads() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        std::fs::create_dir_all(repo.root.join("spec")).unwrap();
        std::fs::write(repo.root.join("spec/health.md"), "# health\n").unwrap();

        let prompt = with(
            common_fields("conv-1"),
            json!({
                "prompt": "Add a health-check endpoint",
                "attachments": [
                    { "type": "file", "file_path": repo.root.join("spec/health.md").to_string_lossy() },
                    { "type": "file", "file_path": "/elsewhere/notes.md" }
                ]
            }),
        );
        assert_eq!(
            handle_in(&repo, "beforeSubmitPrompt", &prompt).unwrap(),
            json!({ "continue": true })
        );
        // Another conversation's prompt must not leak into conv-1's edit.
        let other = with(
            common_fields("conv-2"),
            json!({ "prompt": "Rename the logger" }),
        );
        handle_in(&repo, "beforeSubmitPrompt", &other).unwrap();

        let edit = pre_tool_then_write(&repo, "conv-1", "src/health.ts", CODE);
        assert_eq!(handle_in(&repo, "afterFileEdit", &edit).unwrap(), json!({}));

        let ev = head_event(&repo);
        assert_eq!(ev.agent.as_deref(), Some("cursor"));
        assert_eq!(ev.model.as_deref(), Some("claude-opus-4-7"));
        assert_eq!(ev.prompt.as_deref(), Some("Add a health-check endpoint"));
        assert_eq!(ev.tool.as_deref(), Some("Write"));
        assert_eq!(ev.message.as_deref(), Some("Write src/health.ts"));
        assert_eq!(ev.writes, vec!["src/health.ts".to_string()]);
        assert_eq!(ev.reads, vec!["spec/health.md".to_string()]);
        assert_eq!(ev.evidence, Some(Evidence::declared("cursor-hook")));
        assert_eq!(
            (ev.tokens_in, ev.tokens_out, ev.cost_usd),
            (None, None, None)
        );
        // The pre-state is the preToolUse snapshot: the diff is exactly this file.
        let store = Store::new(&repo);
        let pre = store.read_snapshot(&ev.pre_snapshot).unwrap();
        let post = store.read_snapshot(&ev.post_snapshot).unwrap();
        assert_ne!(pre.tree, post.tree);
    }

    /// AI writes the file, a human rewrites a line, the file is renamed, a
    /// formatter reindents, then the agent adds a function. The second event
    /// owns the function it added. It does not own the human's line or the
    /// line that survived from the first event. This holds when PreToolUse
    /// captured the gap, and when it did not: the declared edit is reversed
    /// onto the file as it sits, so the gap is on both sides of the diff.
    #[test]
    fn human_rename_formatter_between_agents_is_not_the_second_agents() {
        for capture_pre in [true, false] {
            let tmp = tempfile::tempdir().unwrap();
            let repo = Repo::init(tmp.path()).unwrap();
            let original = "def refresh(user):\n    token = rotate(user)\n    return token\n";
            let first_edit = pre_tool_then_write(&repo, "conv-1", "auth.py", original);
            handle_in(&repo, "afterFileEdit", &first_edit).unwrap();
            let first = repo.head_event().unwrap().unwrap();

            std::fs::write(
                repo.root.join("auth.py"),
                "def refresh(user):\n    return rotate(user)\n",
            )
            .unwrap();
            std::fs::rename(repo.root.join("auth.py"), repo.root.join("session.py")).unwrap();
            let formatted = "def refresh(user):\n        return rotate(user)\n";
            std::fs::write(repo.root.join("session.py"), formatted).unwrap();

            let addition = "\ndef logout(user):\n    return drop(user)\n";
            let abs = repo.root.join("session.py");
            if capture_pre {
                let pre = with(common_fields("conv-1"), json!({ "tool_name": "Write" }));
                handle_in(&repo, "preToolUse", &pre).unwrap();
            }
            std::fs::write(&abs, format!("{formatted}{addition}")).unwrap();
            let second_edit = with(
                common_fields("conv-1"),
                json!({
                    "file_path": abs.to_string_lossy(),
                    "edits": [{ "old_string": "", "new_string": addition }]
                }),
            );
            handle_in(&repo, "afterFileEdit", &second_edit).unwrap();
            let second = repo.head_event().unwrap().unwrap();
            assert_ne!(first, second);

            let store = Store::new(&repo);
            let chain = chain_to(&store, Some(&second)).unwrap();
            let owners = line_owners(&store, &chain, std::path::Path::new("session.py")).unwrap();
            let text = std::fs::read_to_string(&abs).unwrap();
            let lines: Vec<&str> = text.lines().collect();
            assert_eq!(owners.len(), lines.len(), "pre captured: {capture_pre}");
            for (line, owner) in lines.iter().zip(&owners) {
                match *line {
                    "def logout(user):" | "    return drop(user)" => {
                        assert_eq!(
                            owner.as_deref(),
                            Some(second.as_str()),
                            "{line} / pre {capture_pre}"
                        );
                    }
                    "def refresh(user):" | "        return rotate(user)" => {
                        assert_ne!(
                            owner.as_deref(),
                            Some(second.as_str()),
                            "{line} was attributed to the second agent (pre captured: {capture_pre})"
                        );
                    }
                    _ => {}
                }
            }
            // The surviving signature is the first agent's. The rename and
            // the reindent do not move it.
            let signature = lines
                .iter()
                .position(|l| *l == "def refresh(user):")
                .unwrap();
            assert_eq!(
                owners[signature].as_deref(),
                Some(first.as_str()),
                "pre captured: {capture_pre}"
            );
        }
    }

    fn origin(repo: &Repo, rel: &str, line: &str) -> Option<crate::provenance::LineOrigin> {
        let store = Store::new(repo);
        let head = repo.head_event().unwrap();
        find_line_origin(&store, head.as_deref(), std::path::Path::new(rel), line)
            .unwrap()
            .0
    }

    fn assert_declared(ev: &Evidence) {
        assert_eq!(ev.label(), "declared");
        let described = ev.describe();
        assert!(described.starts_with("declared by"), "{described}");
        let lower = described.to_lowercase();
        assert!(!lower.contains("proven"), "{described}");
        assert!(!lower.contains("verified"), "{described}");
        let json = serde_json::to_value(ev).unwrap();
        assert_eq!(json["class"], "declared");
    }

    /// Expected, written first. A runtime declares `old_string: ""` and
    /// `new_string` equal to the whole file, and it skips PreToolUse.
    /// The file already holds an earlier agent's lines plus a human line.
    ///
    /// - `def refresh(user):` stays the first event. Evidence stays declared.
    /// - `# human` is introduced by no event (UNKNOWN).
    /// - The lying call records nothing. A declaration is not line ownership.
    #[test]
    fn wholesale_declaration_does_not_take_lines_the_ledger_already_has() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        let original = "def refresh(user):\n    return token\n";
        let first = pre_tool_then_write(&repo, "conv-a", "auth.py", original);
        handle_in(&repo, "afterFileEdit", &first).unwrap();
        let agent_a = repo.head_event().unwrap().unwrap();

        let human = "def refresh(user):\n    return token\n# human\n";
        std::fs::write(repo.root.join("auth.py"), human).unwrap();
        let abs = repo.root.join("auth.py");
        let lie = with(
            common_fields("conv-lie"),
            json!({
                "file_path": abs.to_string_lossy(),
                "edits": [{ "old_string": "", "new_string": human }],
                "timestamp": "1999-01-01T00:00:00Z"
            }),
        );
        assert_eq!(handle_in(&repo, "afterFileEdit", &lie).unwrap(), json!({}));
        assert_eq!(
            repo.head_event().unwrap().as_deref(),
            Some(agent_a.as_str())
        );

        let kept = origin(&repo, "auth.py", "def refresh(user):").expect("still the first event");
        assert_eq!(kept.id, agent_a);
        assert_declared(kept.event.evidence.as_ref().unwrap());
        assert!(
            origin(&repo, "auth.py", "# human").is_none(),
            "the human line must stay unknown"
        );
        // The planted timestamp is not the event's clock. The first event
        // was recorded in this process, not in 1999.
        assert!(!kept.event.created_at.starts_with("1999"));
    }

    /// Expected: a path the declaration names, and that the tree does not
    /// contain, does not become ownership of a different file a human edited.
    #[test]
    fn declared_path_that_is_not_in_the_tree_does_not_steal_another_file() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        let original = "def refresh(user):\n    return token\n";
        let first = pre_tool_then_write(&repo, "conv-a", "auth.py", original);
        handle_in(&repo, "afterFileEdit", &first).unwrap();
        let agent_a = repo.head_event().unwrap().unwrap();

        std::fs::write(
            repo.root.join("auth.py"),
            "def refresh(user):\n    return token\n# human\n",
        )
        .unwrap();
        let lie = with(
            common_fields("conv-lie"),
            json!({
                "file_path": repo.root.join("other.py").to_string_lossy(),
                "edits": [{ "old_string": "", "new_string": "print('nope')\n" }]
            }),
        );
        handle_in(&repo, "afterFileEdit", &lie).unwrap();
        assert_eq!(
            repo.head_event().unwrap().as_deref(),
            Some(agent_a.as_str())
        );
        assert!(origin(&repo, "auth.py", "# human").is_none());
        assert_eq!(
            origin(&repo, "auth.py", "def refresh(user):").unwrap().id,
            agent_a
        );
    }

    /// Expected: the first time the ledger sees a path, a creation
    /// declaration is recorded. Evidence is declared. Nothing in the ledger
    /// contradicts it, and that is not a proof a human did not type it first.
    #[test]
    fn first_observation_of_a_path_stays_declared() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        let body = "def refresh(user):\n    return token\n";
        let abs = repo.root.join("auth.py");
        std::fs::write(&abs, body).unwrap();
        let edit = with(
            common_fields("conv-a"),
            json!({
                "file_path": abs.to_string_lossy(),
                "edits": [{ "old_string": "", "new_string": body }]
            }),
        );
        handle_in(&repo, "afterFileEdit", &edit).unwrap();
        let ev = head_event(&repo);
        assert_declared(ev.evidence.as_ref().unwrap());
        let id = repo.head_event().unwrap().unwrap();
        let store = Store::new(&repo);
        let chain = chain_to(&store, Some(&id)).unwrap();
        let owners = line_owners(&store, &chain, std::path::Path::new("auth.py")).unwrap();
        assert!(owners.iter().all(|o| o.as_deref() == Some(id.as_str())));
    }

    /// Expected attribution of `final.py` after
    /// AI-A → human line → AI-B → two renames → formatter → AI-C
    /// (AI-C has no PreToolUse; its edit is the appended function only):
    ///
    /// - `def refresh(user):` → AI-A
    /// - `        return token` → UNKNOWN (the formatter changed the indent)
    /// - `# human` → UNKNOWN
    /// - `def ping():` / `    return 1` → AI-B
    /// - `def logout(user):` / `    return drop(user)` → AI-C
    ///
    /// AI-C owns none of the earlier lines. A shell is not in this chain:
    /// a shell that declares no edit still falls back to the previous
    /// snapshot, and that result is not asserted as correct.
    #[test]
    fn two_renames_and_a_formatter_between_three_agents_keep_each_edit() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        let original = "def refresh(user):\n    return token\n";
        let a = pre_tool_then_write(&repo, "conv-a", "auth.py", original);
        handle_in(&repo, "afterFileEdit", &a).unwrap();
        let agent_a = repo.head_event().unwrap().unwrap();

        let with_human = "def refresh(user):\n    return token\n# human\n";
        std::fs::write(repo.root.join("auth.py"), with_human).unwrap();

        let ping = "def ping():\n    return 1\n";
        let with_b = format!("{with_human}{ping}");
        let abs = repo.root.join("auth.py");
        handle_in(
            &repo,
            "preToolUse",
            &with(common_fields("conv-b"), json!({ "tool_name": "Write" })),
        )
        .unwrap();
        std::fs::write(&abs, &with_b).unwrap();
        handle_in(
            &repo,
            "afterFileEdit",
            &with(
                common_fields("conv-b"),
                json!({
                    "file_path": abs.to_string_lossy(),
                    "edits": [{ "old_string": "", "new_string": ping }]
                }),
            ),
        )
        .unwrap();
        let agent_b = repo.head_event().unwrap().unwrap();

        std::fs::rename(repo.root.join("auth.py"), repo.root.join("mid.py")).unwrap();
        std::fs::rename(repo.root.join("mid.py"), repo.root.join("final.py")).unwrap();
        let formatted =
            "def refresh(user):\n        return token\n# human\ndef ping():\n    return 1\n";
        std::fs::write(repo.root.join("final.py"), formatted).unwrap();

        let logout = "def logout(user):\n    return drop(user)\n";
        let final_abs = repo.root.join("final.py");
        std::fs::write(&final_abs, format!("{formatted}{logout}")).unwrap();
        handle_in(
            &repo,
            "afterFileEdit",
            &with(
                common_fields("conv-c"),
                json!({
                    "file_path": final_abs.to_string_lossy(),
                    "edits": [{ "old_string": "", "new_string": logout }]
                }),
            ),
        )
        .unwrap();
        let agent_c = repo.head_event().unwrap().unwrap();

        let store = Store::new(&repo);
        let chain = chain_to(&store, Some(&agent_c)).unwrap();
        let owners = line_owners(&store, &chain, std::path::Path::new("final.py")).unwrap();
        let text = std::fs::read_to_string(&final_abs).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(owners.len(), lines.len());
        for (line, owner) in lines.iter().zip(&owners) {
            let want: Option<&str> = match *line {
                "def refresh(user):" => Some(agent_a.as_str()),
                "        return token" | "# human" => None,
                "def ping():" | "    return 1" => Some(agent_b.as_str()),
                "def logout(user):" | "    return drop(user)" => Some(agent_c.as_str()),
                _ => panic!("unexpected line {line:?}"),
            };
            assert_eq!(owner.as_deref(), want, "{line}");
        }
        let c_ev = store.read_event(&agent_c).unwrap();
        assert_declared(c_ev.evidence.as_ref().unwrap());
    }

    /// Expected: two conversations, one file, each with PreToolUse.
    /// Each owns the function it appended. Neither owns the other's lines.
    #[test]
    fn two_agents_on_one_file_keep_their_own_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        let alpha = "def alpha():\n    return 1\n";
        let first = pre_tool_then_write(&repo, "conv-1", "auth.py", alpha);
        handle_in(&repo, "afterFileEdit", &first).unwrap();
        let agent_a = repo.head_event().unwrap().unwrap();

        let beta = "def beta():\n    return 2\n";
        let abs = repo.root.join("auth.py");
        handle_in(
            &repo,
            "preToolUse",
            &with(common_fields("conv-2"), json!({ "tool_name": "Write" })),
        )
        .unwrap();
        std::fs::write(&abs, format!("{alpha}{beta}")).unwrap();
        handle_in(
            &repo,
            "afterFileEdit",
            &with(
                common_fields("conv-2"),
                json!({
                    "file_path": abs.to_string_lossy(),
                    "edits": [{ "old_string": "", "new_string": beta }]
                }),
            ),
        )
        .unwrap();
        let agent_b = repo.head_event().unwrap().unwrap();
        let store = Store::new(&repo);
        let chain = chain_to(&store, Some(&agent_b)).unwrap();
        let owners = line_owners(&store, &chain, std::path::Path::new("auth.py")).unwrap();
        let text = std::fs::read_to_string(&abs).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        for (line, owner) in lines.iter().zip(&owners) {
            let want = if line.contains("alpha") || *line == "    return 1" {
                Some(agent_a.as_str())
            } else if line.contains("beta") || *line == "    return 2" {
                Some(agent_b.as_str())
            } else {
                panic!("{line}");
            };
            assert_eq!(owner.as_deref(), want, "{line}");
        }
    }

    /// Expected: a copy, a split and a merge that no hook observed are
    /// unknown on the new path. Delete-and-recreate of the same bytes is
    /// not observed either, so the earlier event still answers for that
    /// text. That is a limit of an unobserved delete, not continuous authorship.
    #[test]
    fn unobserved_copy_split_merge_and_recreate_are_not_invented() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        let body = "def refresh(user):\n    return token\n";
        let first = pre_tool_then_write(&repo, "conv-a", "auth.py", body);
        handle_in(&repo, "afterFileEdit", &first).unwrap();
        let agent_a = repo.head_event().unwrap().unwrap();

        std::fs::copy(repo.root.join("auth.py"), repo.root.join("copy.py")).unwrap();
        assert!(origin(&repo, "copy.py", "def refresh(user):").is_none());

        std::fs::write(repo.root.join("left.py"), "def refresh(user):\n").unwrap();
        std::fs::write(repo.root.join("right.py"), "    return token\n").unwrap();
        assert!(origin(&repo, "left.py", "def refresh(user):").is_none());
        assert!(origin(&repo, "right.py", "    return token").is_none());

        std::fs::write(
            repo.root.join("merged.py"),
            "def refresh(user):\n    return token\n",
        )
        .unwrap();
        assert!(origin(&repo, "merged.py", "def refresh(user):").is_none());

        std::fs::remove_file(repo.root.join("auth.py")).unwrap();
        std::fs::write(repo.root.join("auth.py"), body).unwrap();
        let again = origin(&repo, "auth.py", "def refresh(user):").unwrap();
        assert_eq!(again.id, agent_a);
        assert_declared(again.event.evidence.as_ref().unwrap());
    }

    /// Expected, written first. A shell declares a command and no edit.
    /// PreToolUse did not leave a snapshot (the pending pre was discarded).
    /// The file already holds an earlier agent's line plus a human rewrite.
    ///
    /// - `def refresh(user):` stays the first event.
    /// - `    return rewritten` is introduced by no event (UNKNOWN).
    /// - No shell event is recorded. Inheriting the previous snapshot would
    ///   turn the gap into the shell's diff, and the command string is not
    ///   evidence of which lines the command wrote.
    #[test]
    fn shell_without_a_captured_pre_does_not_inherit_the_gap() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        let original = "def refresh(user):\n    return token\n";
        let first = pre_tool_then_write(&repo, "conv-a", "auth.py", original);
        handle_in(&repo, "afterFileEdit", &first).unwrap();
        let agent_a = repo.head_event().unwrap().unwrap();

        std::fs::write(
            repo.root.join("auth.py"),
            "def refresh(user):\n    return rewritten\n",
        )
        .unwrap();
        pre_tool_shell(&repo, "conv-sh");
        let pending = repo.dir.join("capture").join("pending-pre.jsonl");
        if pending.exists() {
            std::fs::remove_file(&pending).unwrap();
        }
        let generated = with(
            common_fields("conv-sh"),
            json!({ "command": "rewrite auth.py", "output": "", "duration": 4, "sandbox": false }),
        );
        handle_in(&repo, "afterShellExecution", &generated).unwrap();

        assert_eq!(
            repo.head_event().unwrap().as_deref(),
            Some(agent_a.as_str())
        );
        let kept = origin(&repo, "auth.py", "def refresh(user):").unwrap();
        assert_eq!(kept.id, agent_a);
        assert_declared(kept.event.evidence.as_ref().unwrap());
        assert!(
            origin(&repo, "auth.py", "    return rewritten").is_none(),
            "the rewritten line must stay unknown"
        );
    }

    /// Expected: PreToolUse ran after the human rewrite and before the shell
    /// appended one line. The shell owns that line. It does not own the
    /// human rewrite, and it does not own the first agent's signature.
    #[test]
    fn shell_with_a_captured_pre_owns_only_the_change_after_the_snapshot() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        let original = "def refresh(user):\n    return token\n";
        let first = pre_tool_then_write(&repo, "conv-a", "auth.py", original);
        handle_in(&repo, "afterFileEdit", &first).unwrap();
        let agent_a = repo.head_event().unwrap().unwrap();

        let rewritten = "def refresh(user):\n    return rewritten\n";
        std::fs::write(repo.root.join("auth.py"), rewritten).unwrap();
        pre_tool_shell(&repo, "conv-sh");
        let after = "def refresh(user):\n    return rewritten\ndef ping():\n    return 1\n";
        std::fs::write(repo.root.join("auth.py"), after).unwrap();
        let generated = with(
            common_fields("conv-sh"),
            json!({ "command": "printf ping >> auth.py", "output": "", "duration": 4, "sandbox": false }),
        );
        handle_in(&repo, "afterShellExecution", &generated).unwrap();
        let shell = repo.head_event().unwrap().unwrap();
        assert_ne!(shell, agent_a);
        let ev = head_event(&repo);
        assert_eq!(ev.tool.as_deref(), Some("Shell"));
        assert_declared(ev.evidence.as_ref().unwrap());

        let store = Store::new(&repo);
        let chain = chain_to(&store, Some(&shell)).unwrap();
        let owners = line_owners(&store, &chain, std::path::Path::new("auth.py")).unwrap();
        let text = std::fs::read_to_string(repo.root.join("auth.py")).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(owners.len(), lines.len());
        for (line, owner) in lines.iter().zip(&owners) {
            let want = match *line {
                "def refresh(user):" => Some(agent_a.as_str()),
                "    return rewritten" => None,
                "def ping():" | "    return 1" => Some(shell.as_str()),
                _ => panic!("unexpected line {line:?}"),
            };
            assert_eq!(owner.as_deref(), want, "{line}");
        }
    }

    #[test]
    fn edits_outside_the_repository_and_no_op_shells_record_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        let outside = tempfile::tempdir().unwrap();
        let abs = outside.path().join("other.ts");
        pre_tool_shell(&repo, "conv-1");
        std::fs::write(&abs, CODE).unwrap();
        let edit = with(
            common_fields("conv-1"),
            json!({ "file_path": abs.to_string_lossy(), "edits": [{ "old_string": "", "new_string": CODE }] }),
        );
        assert_eq!(handle_in(&repo, "afterFileEdit", &edit).unwrap(), json!({}));
        assert!(repo.head_event().unwrap().is_none());

        // A command that left the tree as it was is not an event.
        pre_tool_shell(&repo, "conv-1");
        let ls = with(
            common_fields("conv-1"),
            json!({ "command": "ls -la", "output": "total 0\n", "duration": 12, "sandbox": false }),
        );
        assert_eq!(
            handle_in(&repo, "afterShellExecution", &ls).unwrap(),
            json!({})
        );
        assert!(repo.head_event().unwrap().is_none());

        // One that changed it is, with the exact command as its message.
        pre_tool_shell(&repo, "conv-1");
        std::fs::write(repo.root.join("generated.txt"), "made by a script\n").unwrap();
        let generated = with(
            common_fields("conv-1"),
            json!({ "command": "./scripts/gen.sh > generated.txt", "output": "", "duration": 40, "sandbox": false }),
        );
        handle_in(&repo, "afterShellExecution", &generated).unwrap();
        let ev = head_event(&repo);
        assert_eq!(ev.tool.as_deref(), Some("Shell"));
        assert_eq!(
            ev.message.as_deref(),
            Some("./scripts/gen.sh > generated.txt")
        );
        assert_eq!(ev.agent.as_deref(), Some("cursor"));
        assert!(ev.writes.is_empty());
    }

    #[test]
    fn agent_response_becomes_an_exchange_the_next_edit_can_claim() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        let prompt = with(
            common_fields("conv-1"),
            json!({ "prompt": "Add a health-check endpoint" }),
        );
        handle_in(&repo, "beforeSubmitPrompt", &prompt).unwrap();
        let resp = with(
            common_fields("conv-1"),
            json!({ "text": format!("Adding src/health.ts:\n```ts\n{CODE}```") }),
        );
        handle_in(&repo, "afterAgentResponse", &resp).unwrap();

        let exchanges = load_unclaimed_exchanges_since(&repo, 0).unwrap();
        assert_eq!(exchanges.len(), 1);
        assert_eq!(exchanges[0].agent.as_deref(), Some("cursor"));
        assert_eq!(exchanges[0].model.as_deref(), Some("claude-opus-4-7"));
        assert_eq!(
            exchanges[0].prompt.as_deref(),
            Some("Add a health-check endpoint")
        );
        assert!(exchanges[0].response_text.contains("return { sha: SHA"));
        assert_eq!(exchanges[0].tokens_in, None);

        let edit = pre_tool_then_write(&repo, "conv-1", "src/health.ts", CODE);
        handle_in(&repo, "afterFileEdit", &edit).unwrap();
        // The lines it wrote were in its answer: the exchange is spent.
        assert!(load_unclaimed_exchanges_since(&repo, 0).unwrap().is_empty());
        let ev = head_event(&repo);
        assert_eq!(ev.model.as_deref(), Some("claude-opus-4-7"));
        assert_eq!(ev.evidence, Some(Evidence::declared("cursor-hook")));
    }

    #[test]
    fn stop_discards_only_this_conversations_pending_pre_states() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        for conv in ["conv-1", "conv-2"] {
            handle_in(
                &repo,
                "preToolUse",
                &with(common_fields(conv), json!({ "tool_name": "Shell" })),
            )
            .unwrap();
        }
        let pending = repo.dir.join("capture").join("pending-pre.jsonl");
        let before = std::fs::read_to_string(&pending).unwrap();
        assert_eq!(before.lines().count(), 2);

        let stop = with(
            common_fields("conv-1"),
            json!({ "status": "completed", "loop_count": 0 }),
        );
        assert_eq!(handle_in(&repo, "stop", &stop).unwrap(), json!({}));
        let left = std::fs::read_to_string(&pending).unwrap();
        assert_eq!(left.lines().count(), 1);
        assert!(left.contains("conv-2"));

        // A human edits between the turns; the next turn's edit must not
        // absorb it. Its pre-state is the fresh preToolUse snapshot, which
        // already contains the human's file — not the one stop dropped.
        std::fs::write(repo.root.join("human.txt"), "typed by hand\n").unwrap();
        let edit = pre_tool_then_write(&repo, "conv-1", "a.txt", "hello\n");
        handle_in(&repo, "afterFileEdit", &edit).unwrap();
        let ev = head_event(&repo);
        assert_eq!(ev.writes, vec!["a.txt".to_string()]);
        assert!(!before.contains(&ev.pre_snapshot));
        let store = Store::new(&repo);
        let pre = crate::snapshot::flatten_tree(
            &store,
            &store.read_snapshot(&ev.pre_snapshot).unwrap().tree,
        )
        .unwrap();
        assert!(pre.contains_key(std::path::Path::new("human.txt")));
        assert!(!pre.contains_key(std::path::Path::new("a.txt")));
    }

    #[test]
    fn session_start_answers_with_json_even_on_a_fresh_repository() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        let start = with(
            common_fields("conv-1"),
            json!({ "session_id": "conv-1", "is_background_agent": false, "composer_mode": "agent" }),
        );
        assert_eq!(handle_in(&repo, "sessionStart", &start).unwrap(), json!({}));
        assert!(handle_in(&repo, "afterAgentThought", &start).is_err());
        assert_eq!(
            run_event("afterAgentThought", &start.to_string()),
            json!({})
        );
    }
}
