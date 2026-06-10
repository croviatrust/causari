use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::PathBuf;

use crate::cli::McpArgs;
use crate::object::{Event, Snapshot};
use crate::repo::Repo;
use crate::snapshot::{flatten_tree, snapshot_workspace};
use crate::store::Store;

/// `re mcp` — start an MCP (Model Context Protocol) server on stdio.
///
/// Once registered in an agent runtime (Claude Desktop, Claude Code, Cursor,
/// Cline, Windsurf, …) the agent can call Causari tools directly:
///
/// - `causari_record`  — record an event from inside the agent's loop
/// - `causari_recall`  — find verified past actions similar to the current task
/// - `causari_why`     — provenance for a specific line of code
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
                "description": "Record an agent action into the Causari ledger. \
                    Causari will snapshot the workspace, hash it, and store an immutable \
                    event with the prompt, model, tool, reads, writes and reasoning you provide. \
                    Call this AFTER you finish each tool call so the action is captured.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "message":   { "type": "string", "description": "Short human-readable summary of what was just done." },
                        "tool":      { "type": "string", "description": "Which tool you used (e.g. edit_file, run_command)." },
                        "agent":     { "type": "string", "description": "Your agent name." },
                        "model":     { "type": "string", "description": "Underlying model id." },
                        "prompt":    { "type": "string", "description": "The user prompt that triggered this action." },
                        "reasoning": { "type": "string", "description": "Your chain-of-thought, if you can expose it." },
                        "session":   { "type": "string", "description": "Named session to record onto (one per agent enables safe concurrent recording). Created on first use." },
                        "reads":     { "type": "array", "items": { "type": "string" }, "description": "Files you read or considered as context." },
                        "writes":    { "type": "array", "items": { "type": "string" }, "description": "Files you wrote or modified." }
                    },
                    "required": ["message"]
                }
            },
            {
                "name": "causari_recall",
                "description": "Recall proven experience before acting. Searches the signed skill \
                    library first (skills are distilled, Ed25519-signed units of verified past \
                    work, ranked by trust: proven > verified > recorded), then raw ledger events. \
                    Use this BEFORE acting on a task that looks similar to something you (or \
                    another agent) may have done before — it is how you avoid repeating mistakes.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Free-text description of the task or problem." },
                        "limit": { "type": "integer", "description": "Max number of results (default 5)." }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "causari_why",
                "description": "Get the full provenance of a specific source line: the agent, model, \
                    prompt and reasoning that introduced it. Useful before modifying code you did \
                    not write yourself.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "file": { "type": "string", "description": "Path to the file (relative to repo root)." },
                        "line": { "type": "integer", "description": "1-indexed line number." }
                    },
                    "required": ["file", "line"]
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
        exit_code: args
            .get("exit_code")
            .and_then(|v| v.as_i64())
            .map(|n| n as i32),
        created_at: Utc::now().to_rfc3339(),
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
    let store = Store::new(&repo);
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

    // 1. SKILLS first — distilled, signed experience outranks raw events.
    //    Every recall bumps the skill's use counter, which is how a verified
    //    skill earns the ★ proven trust level over time.
    let skills = crate::skill::load_skills(&repo)?;
    let mut skill_hits: Vec<(usize, &String, &crate::skill::SkillEnvelope)> = skills
        .iter()
        .filter(|(_, env)| crate::skill::verify_envelope(env).is_ok())
        .map(|(id, env)| (crate::skill::score_skill(env, &terms), id, env))
        .filter(|(score, _, _)| *score > 0)
        .collect();
    skill_hits.sort_by_key(|h| std::cmp::Reverse(h.0));

    if !skill_hits.is_empty() {
        out.push_str(&format!(
            "# {} skill(s) match {:?} (signed, trust-ranked)\n",
            skill_hits.len(),
            query
        ));
        for (score, id, env) in skill_hits.iter().take(limit) {
            let trust = env.trust();
            out.push_str(&format!(
                "\n## [{}] {} {} — {}\n",
                score,
                trust.badge(),
                trust.as_str(),
                env.skill.title
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
                "- evidence: exit_zero={} survived={} uses={}\n",
                env.skill.verification.exit_zero, env.skill.verification.survived, env.stats.uses
            ));
            let _ = crate::skill::record_use(&repo, id);
        }
        out.push('\n');
    }

    // 2. Raw events from the metadata index (all sessions, one read).
    let indexed = crate::index::ensure(&repo, &store)?;
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
    use similar::{ChangeTag, TextDiff};

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

    let mut cur = repo.head_event()?;
    while let Some(id) = cur {
        let ev = store.read_event(&id)?;
        let pre_snap = store.read_snapshot(&ev.pre_snapshot)?;
        let post_snap = store.read_snapshot(&ev.post_snapshot)?;
        let pre_tree = flatten_tree(&store, &pre_snap.tree)?;
        let post_tree = flatten_tree(&store, &post_snap.tree)?;
        let post_text = match post_tree.get(&rel) {
            Some(id) => String::from_utf8(store.read_blob(id)?).unwrap_or_default(),
            None => {
                cur = ev.parent;
                continue;
            }
        };
        let pre_text = match pre_tree.get(&rel) {
            Some(id) => String::from_utf8(store.read_blob(id)?).unwrap_or_default(),
            None => String::new(),
        };
        if pre_text == post_text {
            cur = ev.parent;
            continue;
        }
        let appears = post_text.lines().any(|l| l == target);
        if !appears {
            cur = ev.parent;
            continue;
        }
        let pre_has = pre_text.lines().any(|l| l == target);
        let introduced = if !pre_has {
            true
        } else {
            TextDiff::from_lines(&pre_text, &post_text)
                .iter_all_changes()
                .any(|c| c.tag() == ChangeTag::Insert && c.value().trim_end_matches('\n') == target)
        };
        if introduced {
            let mut out = format!("# {}:{}\n```\n{}\n```\n\n", file, line_no, target);
            out.push_str(&format!("Introduced by event `{}`\n", &id[..10]));
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
            return Ok(out);
        }
        cur = ev.parent;
    }
    Ok(format!(
        "no recorded event introduced {}:{} (the line predates the first `re record`)",
        file, line_no
    ))
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
  causari_recall  - find past similar events before acting
  causari_why     - explain the provenance of a line of code

Tip: have the agent call `causari_record` after every tool call. Causari will
build a complete, queryable history of the session for you.
"#,
        exe = exe
    );
    Ok(())
}
