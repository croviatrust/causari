use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::PathBuf;

use crate::cli::McpArgs;
use crate::object::{Event, Snapshot};
use crate::repo::Repo;
use crate::snapshot::snapshot_workspace;
use crate::store::Store;

/// `re mcp` — start an MCP (Model Context Protocol) server on stdio.
///
/// Once registered in an agent runtime (Claude Desktop, Claude Code, Cursor,
/// Cline, Windsurf, …) the agent can call Causari tools directly:
///
/// - `causari_record`  — append a declared action to the local ledger
/// - `causari_recall`  — search local skills and events; a search does not change trust
/// - `causari_why`     — ledger event for a line; declared is not authorship
///
/// None of the three is `re audit`.
///
/// This is the bridge that turns Causari from a CLI for power users into a
/// silent companion that *every* agent can use without code changes.
///
/// Protocol: JSON-RPC 2.0 over newline-delimited JSON on stdin/stdout
/// (MCP "stdio" transport). Notifications (requests without an `id`) are
/// accepted but not answered.
pub fn run(args: McpArgs) -> Result<()> {
    if args.install {
        return print_install_snippet();
    }

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut line = String::new();

    log_stderr("causari MCP server starting");

    loop {
        line.clear();
        let n = stdin.lock().read_line(&mut line).context("reading stdin")?;
        if n == 0 {
            break; // EOF
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                log_stderr(&format!("invalid JSON-RPC: {}", e));
                continue;
            }
        };

        let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let id = req.get("id").cloned();
        let params = req.get("params").cloned().unwrap_or(json!({}));

        // Notifications (no id) are not answered.
        let is_notification = id.is_none();
        if is_notification && method.starts_with("notifications/") {
            continue;
        }

        let response = match method {
            "initialize" => Some(handle_initialize(&params)),
            "tools/list" => Some(handle_tools_list()),
            "tools/call" => Some(handle_tools_call(&params)),
            "ping" => Some(Ok(json!({}))),
            "shutdown" => Some(Ok(json!({}))),
            _ if is_notification => None,
            _ => Some(Err(format!("method not found: {}", method))),
        };

        if let (Some(result), Some(id)) = (response, id) {
            let envelope = match result {
                Ok(value) => json!({ "jsonrpc": "2.0", "id": id, "result": value }),
                Err(msg) => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": -32601, "message": msg }
                }),
            };
            writeln!(out, "{}", envelope).context("writing stdout")?;
            out.flush()?;
        }
    }

    log_stderr("causari MCP server stopped");
    Ok(())
}

fn log_stderr(msg: &str) {
    eprintln!("[causari-mcp] {}", msg);
}

fn handle_initialize(_params: &Value) -> Result<Value, String> {
    Ok(json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name": "causari",
            "version": env!("CARGO_PKG_VERSION")
        }
    }))
}

fn handle_tools_list() -> Result<Value, String> {
    Ok(json!({
        "tools": [
            {
                "name": "causari_record",
                "description": "Append one declared action to the local Causari ledger in this \
                    repository, then snapshot the workspace. Causari stores the fields you send. \
                    It does not run a command, and it does not audit git history. This is not \
                    `re audit`.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "message":   { "type": "string", "description": "Short summary you supply of what was just done." },
                        "tool":      { "type": "string", "description": "Tool name you supply (for example edit_file or run_command)." },
                        "agent":     { "type": "string", "description": "Agent name you supply. It is not checked." },
                        "model":     { "type": "string", "description": "Model id you supply. It is not checked." },
                        "prompt":    { "type": "string", "description": "Prompt text you supply." },
                        "reasoning": { "type": "string", "description": "Reasoning text you supply, if you choose to send it." },
                        "session":   { "type": "string", "description": "Named local session to append to. Created on first use." },
                        "reads":     { "type": "array", "items": { "type": "string" }, "description": "Paths you declare as read. Causari does not check that those reads happened." },
                        "writes":    { "type": "array", "items": { "type": "string" }, "description": "Paths you declare as written. Causari does not check that those writes happened." },
                        "tokens_in": { "type": "integer", "minimum": 0, "description": "Non-negative token count you supply for the prompt side. Causari does not measure tokens. A value that is not a non-negative integer is not stored." },
                        "tokens_out": { "type": "integer", "minimum": 0, "description": "Non-negative token count you supply for the completion side. Causari does not measure tokens. A value that is not a non-negative integer is not stored." },
                        "cost_usd":  { "type": "number", "description": "USD figure you supply. Causari does not price the call. A value that is not a number is not stored." },
                        "exit_code": { "type": "integer", "minimum": -2147483648, "maximum": 2147483647, "description": "Exit code supplied by the recorder. Causari does not run the command and does not check that the process exited with this number. Accepted only as an integer from -2147483648 to 2147483647 inclusive. Any other value is rejected and the call records nothing." }
                    },
                    "required": ["message"]
                },
                "annotations": {
                    "readOnlyHint": false,
                    "destructiveHint": false,
                    "idempotentHint": false,
                    "openWorldHint": false
                }
            },
            {
                "name": "causari_recall",
                "description": "Search signed skills and ledger events already stored in this \
                    repository. A search does not record a use and does not change trust. If the \
                    local search index is missing entries, this call appends those entries; it \
                    does not modify skill files. Ed25519 detects a later edit of a skill's signed \
                    core; it is not an outcome and it does not certify the content. `verified` \
                    is a declared signal frozen at distill (a caller-supplied exit code 0, or \
                    every declared write path still at the tip). It is not an observed success \
                    and not the audit field `verified`. A 2× rank weight is that declared signal, \
                    not measured reliability. `recorded` means that signal is absent. `failed` is a \
                    caller-supplied non-zero exit with no exit 0. `proven` is not awarded. A \
                    legacy recall count on the file is not an execution. This is not `re audit`.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Free-text description of the task or problem. An empty query does not search." },
                        "limit": { "type": "integer", "minimum": 0, "description": "Maximum number of skills and of events to return. Default 5." }
                    },
                    "required": ["query"]
                },
                "annotations": {
                    "readOnlyHint": false,
                    "destructiveHint": false,
                    "idempotentHint": true,
                    "openWorldHint": false
                }
            },
            {
                "name": "causari_why",
                "description": "Report the local ledger event recorded against one source line: \
                    agent, model, prompt and evidence class (declared, correlated, or observed). \
                    Declared is what a runtime said; it does not prove who typed the line. A line \
                    with no recorded event is unknown, not human. This reads the ledger and the \
                    file. It is not `re audit`.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "file": { "type": "string", "description": "Path relative to the repository root." },
                        "line": { "type": "integer", "minimum": 1, "description": "1-indexed line number in that file." }
                    },
                    "required": ["file", "line"]
                },
                "annotations": {
                    "readOnlyHint": true,
                    "openWorldHint": false
                }
            }
        ]
    }))
}

fn handle_tools_call(params: &Value) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing tool name".to_string())?;
    let args = params.get("arguments").cloned().unwrap_or(json!({}));

    let text = match name {
        "causari_record" => tool_record(&args).map_err(|e| e.to_string())?,
        "causari_recall" => tool_recall(&args).map_err(|e| e.to_string())?,
        "causari_why" => tool_why(&args).map_err(|e| e.to_string())?,
        other => return Err(format!("unknown tool '{}'", other)),
    };

    Ok(json!({
        "content": [
            { "type": "text", "text": text }
        ]
    }))
}

// ---------- tool implementations ----------

fn tool_record(args: &Value) -> Result<String> {
    // Reject before any snapshot or event write. `try_from` does not narrow.
    let exit_code = crate::object::declared_exit_code(args)?;
    let repo = Repo::discover()?;
    let store = Store::new(&repo);

    let s = |k: &str| args.get(k).and_then(|v| v.as_str()).map(String::from);
    let arr = |k: &str| {
        args.get(k)
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };

    let _lock = repo.lock()?;
    let session = s("session");
    let parent_id = crate::commit::resolve_parent(&repo, session.as_deref())?;
    let pre_snapshot_id = crate::commit::resolve_pre_snapshot(&repo, &store, &parent_id)?;
    let post_tree = snapshot_workspace(&repo)?;
    let post_snapshot_id = store.write_snapshot(&Snapshot {
        tree: post_tree,
        created_at: Utc::now().to_rfc3339(),
    })?;

    let event = Event {
        schema: "causari.event.v0.2".to_string(),
        parent: parent_id.clone(),
        agent: s("agent"),
        model: s("model"),
        tool: s("tool"),
        message: s("message"),
        prompt: s("prompt"),
        reasoning: s("reasoning"),
        reads: arr("reads"),
        writes: arr("writes"),
        tokens_in: args.get("tokens_in").and_then(|v| v.as_u64()),
        tokens_out: args.get("tokens_out").and_then(|v| v.as_u64()),
        cost_usd: args.get("cost_usd").and_then(|v| v.as_f64()),
        pre_snapshot: pre_snapshot_id,
        post_snapshot: post_snapshot_id,
        exit_code,
        created_at: Utc::now().to_rfc3339(),
        evidence: Some(crate::object::Evidence::declared("mcp")),
        redactions: 0,
    };
    let id = crate::commit::commit_event(&repo, &store, &event, session.as_deref())?;
    Ok(format!(
        "recorded event {} — {}",
        &id[..10],
        event.message.unwrap_or_else(|| "(no message)".to_string())
    ))
}

fn tool_recall(args: &Value) -> Result<String> {
    let repo = Repo::discover()?;
    recall_in(&repo, args)
}

fn recall_in(repo: &Repo, args: &Value) -> Result<String> {
    let store = Store::new(repo);
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(5);

    if query.is_empty() {
        return Ok("recall: empty query".to_string());
    }
    let terms: Vec<String> = query.split_whitespace().map(String::from).collect();
    let mut out = String::new();

    // Skills first. A search reads them and does not write their counters.
    let skills = crate::skill::load_admissible_skills(repo)?;
    let mut skill_hits: Vec<(usize, &String, &crate::skill::SkillEnvelope)> = skills
        .iter()
        .map(|(id, env)| (crate::skill::score_skill(env, &terms), id, env))
        .filter(|(score, _, _)| *score > 0)
        .collect();
    skill_hits.sort_by_key(|h| std::cmp::Reverse(h.0));

    if !skill_hits.is_empty() {
        out.push_str(&format!(
            "# {} skill(s) match {:?} (ranked by the declared signal, not by the signature)\n",
            skill_hits.len(),
            query
        ));
        if skill_hits
            .iter()
            .take(limit)
            .any(|(_, _, env)| !env.is_failed() && env.trust() == crate::skill::Trust::Verified)
        {
            out.push_str(crate::skill::VERIFIED_GLOSS);
            out.push('\n');
        }
        for (score, id, env) in skill_hits.iter().take(limit) {
            let trust = env.trust();
            let (badge, label) = if env.is_failed() {
                ("✗", "FAILED — do not repeat this approach")
            } else {
                (trust.badge(), trust.as_str())
            };
            out.push_str(&format!(
                "\n## [{}] {} {} — {}\n",
                score, badge, label, env.skill.title
            ));
            out.push_str(&format!("- skill: {}\n", &id[..10]));
            if let Some(a) = &env.skill.agent {
                out.push_str(&format!("- agent: {}\n", a));
            }
            out.push_str(&format!("- trigger: {}\n", env.skill.trigger));
            for (i, step) in env.skill.steps.iter().enumerate() {
                out.push_str(&format!(
                    "- step {}: [{}] {}{}\n",
                    i + 1,
                    step.tool.as_deref().unwrap_or("-"),
                    step.message.as_deref().unwrap_or(""),
                    if step.writes.is_empty() {
                        String::new()
                    } else {
                        format!(" -> {}", step.writes.join(", "))
                    }
                ));
            }
            out.push_str(&format!(
                "- declared: exit_zero={} survived={} failed={}\n",
                env.skill.verification.exit_zero,
                env.skill.verification.survived,
                env.skill.verification.failed
            ));
            out.push_str("- observed success: none recorded\n");
            out.push_str(&format!(
                "- legacy recalls: {} (not executions; ignored for trust)\n",
                env.stats.uses
            ));
        }
        out.push('\n');
    }

    // 2. Raw events from the metadata index (all sessions, one read).
    let indexed = crate::index::ensure(repo, &store)?;
    let mut hits: Vec<(usize, String, crate::index::IndexEntry)> = indexed
        .into_iter()
        .map(|(id, entry)| {
            let hay = format!(
                "{} {} {} {}",
                entry.message.clone().unwrap_or_default(),
                entry.prompt.clone().unwrap_or_default(),
                entry.reasoning.clone().unwrap_or_default(),
                entry.tool.clone().unwrap_or_default()
            )
            .to_lowercase();
            let score: usize = terms.iter().map(|t| hay.matches(t.as_str()).count()).sum();
            (score, id, entry)
        })
        .filter(|(score, _, _)| *score > 0)
        .collect();
    hits.sort_by(|a, b| (b.0, &b.2.created_at).cmp(&(a.0, &a.2.created_at)));

    if skill_hits.is_empty() && hits.is_empty() {
        return Ok(format!("no skills or past events match {:?}", query));
    }

    if !hits.is_empty() {
        out.push_str(&format!("# {} event(s) match {:?}\n", hits.len(), query));
        for (score, id, entry) in hits.iter().take(limit) {
            out.push_str(&format!(
                "\n## [{score}] event {short}\n",
                score = score,
                short = &id[..10]
            ));
            if let Some(a) = &entry.agent {
                out.push_str(&format!("- agent: {}\n", a));
            }
            if let Some(m) = &entry.message {
                out.push_str(&format!("- message: {}\n", m));
            }
            if let Some(p) = &entry.prompt {
                out.push_str(&format!("- prompt: {}\n", p));
            }
            if let Some(r) = &entry.reasoning {
                out.push_str(&format!("- reasoning: {}\n", r));
            }
        }
    }
    Ok(out)
}

fn tool_why(args: &Value) -> Result<String> {
    let repo = Repo::discover()?;
    let store = Store::new(&repo);

    let file = args
        .get("file")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing 'file'"))?
        .replace('\\', "/");
    let line_no = args
        .get("line")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow::anyhow!("missing 'line'"))? as usize;
    let rel = PathBuf::from(&file);

    let abs = repo.root.join(&rel);
    let current =
        std::fs::read_to_string(&abs).with_context(|| format!("reading {}", abs.display()))?;
    let lines: Vec<&str> = current.lines().collect();
    if line_no == 0 || line_no > lines.len() {
        return Ok(format!(
            "{} only has {} lines (asked for line {})",
            file,
            lines.len(),
            line_no
        ));
    }
    let target = lines[line_no - 1].to_string();

    let head = repo.head_event()?;
    let (origin, _) = crate::provenance::find_line_origin(&store, head.as_deref(), &rel, &target)?;
    match origin {
        Some(o) => {
            let (id, ev) = (o.id, o.event);
            let mut out = format!("# {}:{}\n```\n{}\n```\n\n", file, line_no, target);
            out.push_str(&format!("Introduced by event `{}`\n", &id[..10]));
            out.push_str(&format!(
                "- evidence: {}\n",
                ev.evidence
                    .as_ref()
                    .map(|e| e.describe())
                    .unwrap_or_else(|| "unrecorded (older event)".to_string())
            ));
            if ev.parent.is_none() {
                out.push_str(
                    "- note: root event — the line was present when recording started; the agent named may not have written it\n",
                );
            }
            if let Some(a) = &ev.agent {
                out.push_str(&format!("- agent: {}\n", a));
            }
            if let Some(m) = &ev.model {
                out.push_str(&format!("- model: {}\n", m));
            }
            if let Some(t) = &ev.tool {
                out.push_str(&format!("- tool: {}\n", t));
            }
            if let Some(m) = &ev.message {
                out.push_str(&format!("- message: {}\n", m));
            }
            if let Some(p) = &ev.prompt {
                out.push_str(&format!("- prompt: {}\n", p));
            }
            if let Some(r) = &ev.reasoning {
                out.push_str(&format!("- reasoning: {}\n", r));
            }
            Ok(out)
        }
        None => Ok(format!(
            "no recorded event introduced {}:{} (the line predates the first `re record`, or was written without a recorder running)",
            file, line_no
        )),
    }
}

fn print_install_snippet() -> Result<()> {
    let raw_exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "re".to_string());
    // JSON config files require backslashes to be escaped; do it for the user.
    let exe = raw_exe.replace('\\', "\\\\");
    println!(
        r#"Add the following to your agent runtime's MCP server configuration.

# Claude Desktop  (%APPDATA%/Claude/claude_desktop_config.json on Windows,
#                  ~/Library/Application Support/Claude/claude_desktop_config.json on macOS)
{{
  "mcpServers": {{
    "causari": {{
      "command": "{exe}",
      "args": ["mcp"],
      "cwd": "<absolute path to your project>"
    }}
  }}
}}

# Cursor / Windsurf: same shape, the editor will surface the tools automatically.
# Cline (VS Code):   add the same entry to its `cline_mcp_settings.json`.

The agent then has three new tools:
  causari_record  - record one of its own actions into the ledger
  causari_recall  - search local skills and events; a search does not change trust; not re audit
  causari_why     - ledger event for a line; declared is not authorship; not re audit

Tip: have the agent call `causari_record` after every tool call. Causari will
build a complete, queryable history of the session for you.
"#,
        exe = exe
    );
    Ok(())
}

#[cfg(test)]
mod wording_tests {
    use super::*;

    #[test]
    fn tool_descriptions_match_what_the_handlers_do() {
        let list = handle_tools_list().expect("tools");
        let tools = list["tools"].as_array().expect("array");
        let tool = |name: &str| {
            tools
                .iter()
                .find(|t| t["name"] == name)
                .unwrap_or_else(|| panic!("missing {name}"))
        };

        let record = tool("causari_record");
        let props = &record["inputSchema"]["properties"];
        for key in [
            "message",
            "tool",
            "agent",
            "model",
            "prompt",
            "reasoning",
            "session",
            "reads",
            "writes",
            "tokens_in",
            "tokens_out",
            "cost_usd",
            "exit_code",
        ] {
            assert!(props.get(key).is_some(), "schema omits {key}");
        }
        let exit_code = props["exit_code"]["description"].as_str().unwrap();
        assert!(exit_code.contains("supplied by the recorder"));
        assert!(exit_code.contains("does not run the command"));
        assert!(exit_code.contains("records nothing"));
        assert_eq!(props["exit_code"]["minimum"], i32::MIN);
        assert_eq!(props["exit_code"]["maximum"], i32::MAX);
        assert_eq!(props["tokens_in"]["type"], "integer");
        assert_eq!(props["tokens_in"]["minimum"], 0);
        assert_eq!(props["tokens_out"]["type"], "integer");
        assert_eq!(props["tokens_out"]["minimum"], 0);
        assert_eq!(props["cost_usd"]["type"], "number");
        let record_desc = record["description"].as_str().unwrap();
        assert!(record_desc.contains("not `re audit`"));
        assert_eq!(record["annotations"]["readOnlyHint"], false);
        assert_eq!(record["annotations"]["destructiveHint"], false);
        assert_eq!(record["annotations"]["idempotentHint"], false);
        assert_eq!(record["annotations"]["openWorldHint"], false);

        let recall = tool("causari_recall");
        let desc = recall["description"].as_str().unwrap();
        assert!(desc.contains("`proven` is not awarded"));
        assert!(desc.contains("does not change trust"));
        assert!(desc.contains("not an execution"));
        assert!(desc.contains("not the audit field `verified`"));
        assert!(desc.contains("not `re audit`"));
        assert!(desc.contains("does not certify the content"));
        assert!(desc.contains("not measured reliability"));
        assert!(!desc.contains("at least 3"));
        assert_eq!(recall["annotations"]["readOnlyHint"], false);
        assert_eq!(recall["annotations"]["destructiveHint"], false);
        assert_eq!(recall["annotations"]["idempotentHint"], true);
        assert_eq!(recall["annotations"]["openWorldHint"], false);

        let why = tool("causari_why");
        let why_desc = why["description"].as_str().unwrap();
        assert!(why_desc.contains("does not prove who typed the line"));
        assert!(why_desc.contains("not `re audit`"));
        assert_eq!(why["annotations"]["readOnlyHint"], true);
        assert_eq!(why["annotations"]["openWorldHint"], false);
        assert!(why["annotations"].get("destructiveHint").is_none());
    }

    #[test]
    fn repeated_recalls_do_not_promote_and_match_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        let key = crate::skill::load_or_create_signing_key(&repo).unwrap();

        let declared = skill_core("paint the note blue", true, true, false);
        let mut promoted = crate::skill::sign_skill(declared, &key).unwrap();
        promoted.stats.uses = 7;
        let promoted_id = crate::skill::skill_id(&promoted.skill).unwrap();
        crate::skill::save_skill(&repo, &promoted_id, &promoted).unwrap();

        let failed = skill_core("break the note on purpose", false, false, true);
        let mut failed_env = crate::skill::sign_skill(failed, &key).unwrap();
        failed_env.stats.uses = 1;
        let failed_id = crate::skill::skill_id(&failed_env.skill).unwrap();
        crate::skill::save_skill(&repo, &failed_id, &failed_env).unwrap();

        let args = json!({"query": "paint the note blue", "limit": 1});
        let first = recall_in(&repo, &args).unwrap();
        let second = recall_in(&repo, &args).unwrap();
        let third = recall_in(&repo, &args).unwrap();
        assert_eq!(first, second);
        assert_eq!(second, third);
        assert!(first.contains("◆ verified"));
        assert!(first.contains("does not certify the content"));
        assert!(first.contains("not measured reliability"));
        assert!(!first.contains("★ proven"));
        assert!(first.contains("legacy recalls: 7"));
        assert!(first.contains("observed success: none recorded"));
        let (_, after) = crate::skill::find_skill(&repo, &promoted_id).unwrap();
        assert_eq!(after.stats.uses, 7);
        crate::skill::verify_envelope(&after).unwrap();
        assert_eq!(after.trust(), crate::skill::Trust::Verified);

        let failed_text = recall_in(
            &repo,
            &json!({"query": "break the note on purpose", "limit": 1}),
        )
        .unwrap();
        assert!(failed_text.contains("FAILED — do not repeat this approach"));
        assert!(failed_text.contains("failed=true"));
        let (_, failed_after) = crate::skill::find_skill(&repo, &failed_id).unwrap();
        assert_eq!(failed_after.stats.uses, 1);
        crate::skill::verify_envelope(&failed_after).unwrap();
        assert_eq!(failed_after.trust(), crate::skill::Trust::Recorded);
    }

    #[test]
    fn exit_code_outside_i32_is_rejected_and_not_narrowed() {
        assert_eq!(
            crate::object::declared_exit_code(&json!({"exit_code": i32::MAX})).unwrap(),
            Some(i32::MAX)
        );
        assert_eq!(
            crate::object::declared_exit_code(&json!({"exit_code": i32::MIN})).unwrap(),
            Some(i32::MIN)
        );
        assert_eq!(crate::object::declared_exit_code(&json!({})).unwrap(), None);
        assert_eq!(
            crate::object::declared_exit_code(&json!({"exit_code": null})).unwrap(),
            None
        );

        let too_high = tool_record(&json!({
            "message": "x",
            "exit_code": i32::MAX as i64 + 1
        }))
        .unwrap_err()
        .to_string();
        assert!(
            too_high.contains("outside the signed 32-bit range"),
            "{too_high}"
        );
        assert!(
            !too_high.contains("not a causari repository"),
            "rejection must happen before any repository write: {too_high}"
        );

        let too_low = crate::object::declared_exit_code(&json!({"exit_code": i32::MIN as i64 - 1}))
            .unwrap_err()
            .to_string();
        assert!(
            too_low.contains("outside the signed 32-bit range"),
            "{too_low}"
        );

        let fraction = crate::object::declared_exit_code(&json!({"exit_code": 1.5}))
            .unwrap_err()
            .to_string();
        assert!(fraction.contains("must be an integer"), "{fraction}");

        // A non-negative integer token and a numeric cost are read without a narrowing cast.
        let args = json!({"tokens_in": 3_u64, "tokens_out": 4_u64, "cost_usd": 0.5});
        assert_eq!(args.get("tokens_in").and_then(|v| v.as_u64()), Some(3));
        assert_eq!(args.get("tokens_out").and_then(|v| v.as_u64()), Some(4));
        assert_eq!(args.get("cost_usd").and_then(|v| v.as_f64()), Some(0.5));
        assert_eq!(
            json!({"tokens_in": 1.5})
                .get("tokens_in")
                .and_then(|v| v.as_u64()),
            None
        );
    }

    fn skill_core(
        title: &str,
        exit_zero: bool,
        survived: bool,
        failed: bool,
    ) -> crate::skill::SkillCore {
        crate::skill::SkillCore {
            schema: crate::skill::SKILL_SCHEMA.into(),
            title: title.into(),
            trigger: title.into(),
            steps: vec![],
            agent: Some("fixture".into()),
            model: None,
            source_events: vec!["e1".into()],
            files: vec![],
            verification: crate::skill::Verification {
                exit_zero,
                survived,
                failed,
            },
            created_at: "2026-01-01T00:00:00Z".into(),
        }
    }
}
