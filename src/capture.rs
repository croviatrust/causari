use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::repo::Repo;

// The capture layer is what makes Causari's provenance *real* instead of
// self-reported. Two independent streams flow into `.causari/capture/`:
//
// - `exchanges.jsonl` — every LLM request/response seen by `re proxy`
// - `prompts.jsonl`   — every user prompt reported by agent hooks (`re hook`)
//
// `re watch` then performs the **causal join**: when files change on disk,
// the inserted lines are searched *inside* recent completions. If the code
// that appeared in a file also appeared in a model's answer moments before,
// the two are causally linked — prompt, model, tokens and cost get attached
// to the filesystem event. No agent cooperation required.

/// A single LLM request/response captured by `re proxy`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Exchange {
    /// Unix epoch milliseconds when the response completed.
    pub ts_ms: u64,
    /// Best-effort agent identity (from the User-Agent header).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The last user message in the request (the task that drove the call).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// Full assistant text, assembled from SSE deltas when streaming.
    #[serde(default)]
    pub response_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens_in: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens_out: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

/// A user prompt reported by an agent-side hook (e.g. Claude Code).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptRecord {
    pub ts_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub prompt: String,
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn capture_dir(repo: &Repo) -> PathBuf {
    repo.dir.join("capture")
}

pub fn exchanges_path(repo: &Repo) -> PathBuf {
    capture_dir(repo).join("exchanges.jsonl")
}

pub fn prompts_path(repo: &Repo) -> PathBuf {
    capture_dir(repo).join("prompts.jsonl")
}

/// Append one JSON object as a line to an append-only ledger file.
pub fn append_jsonl<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    let line = serde_json::to_string(value)?;
    writeln!(f, "{}", line)?;
    Ok(())
}

/// Load all exchanges captured at or after `since_ms`.
pub fn load_exchanges_since(repo: &Repo, since_ms: u64) -> Result<Vec<Exchange>> {
    let path = exchanges_path(repo);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(&path)?;
    Ok(raw
        .lines()
        .filter_map(|l| serde_json::from_str::<Exchange>(l).ok())
        .filter(|e| e.ts_ms >= since_ms)
        .collect())
}

/// Most recent hook-reported prompt, optionally restricted to a session.
pub fn last_prompt(repo: &Repo, session_id: Option<&str>) -> Result<Option<PromptRecord>> {
    let path = prompts_path(repo);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)?;
    Ok(raw
        .lines()
        .filter_map(|l| serde_json::from_str::<PromptRecord>(l).ok())
        .rfind(|p| match session_id {
            Some(sid) => p.session_id.as_deref() == Some(sid),
            None => true,
        }))
}

// ---------------------------------------------------------------------------
// The correlation engine: content-based causal join
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Correlation {
    pub exchange: Exchange,
    /// Fraction of significant inserted lines found inside the completion.
    pub score: f64,
    pub matched: usize,
    pub considered: usize,
}

/// Minimum trimmed length for a line to count as "significant".
/// Filters out braces, blank-ish lines and one-token noise that would
/// match any completion by accident.
const MIN_SIGNIFICANT_LEN: usize = 6;

/// Minimum score for a correlation to be trusted.
const MIN_SCORE: f64 = 0.25;

/// Given the lines inserted by a filesystem change and the recent LLM
/// exchanges, find the exchange that most plausibly *produced* the change.
///
/// The join is by **content**, not just time: each significant inserted line
/// is searched verbatim inside each completion. Code that an agent wrote to
/// disk almost always appeared first in a model response — that overlap is
/// the causal fingerprint.
pub fn correlate(added_lines: &[String], exchanges: &[Exchange]) -> Option<Correlation> {
    let considered: Vec<&str> = added_lines
        .iter()
        .map(|l| l.trim())
        .filter(|l| l.len() >= MIN_SIGNIFICANT_LEN)
        .collect();
    if considered.is_empty() {
        return None;
    }
    let mut best: Option<Correlation> = None;
    for ex in exchanges {
        if ex.response_text.is_empty() {
            continue;
        }
        let matched = considered
            .iter()
            .filter(|l| ex.response_text.contains(**l))
            .count();
        if matched == 0 {
            continue;
        }
        let score = matched as f64 / considered.len() as f64;
        let better = match &best {
            None => true,
            Some(b) => score > b.score || (score == b.score && ex.ts_ms > b.exchange.ts_ms),
        };
        if better {
            best = Some(Correlation {
                exchange: ex.clone(),
                score,
                matched,
                considered: considered.len(),
            });
        }
    }
    best.filter(|b| b.score >= MIN_SCORE)
}

// ---------------------------------------------------------------------------
// Request / response parsing (OpenAI + Anthropic wire formats)
// ---------------------------------------------------------------------------

/// Extract the last *user* message from an OpenAI `messages` array, an
/// Anthropic `messages` array, or an OpenAI Responses `input` array.
pub fn extract_prompt(body: &Value) -> Option<String> {
    let messages = body
        .get("messages")
        .or_else(|| body.get("input"))?
        .as_array()?;
    for m in messages.iter().rev() {
        if m.get("role").and_then(|r| r.as_str()) != Some("user") {
            continue;
        }
        if let Some(text) = m.get("content").and_then(content_text) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Flatten a content field: plain string, or array of text blocks.
fn content_text(c: &Value) -> Option<String> {
    match c {
        Value::String(s) => Some(s.clone()),
        Value::Array(parts) => {
            let mut out = String::new();
            for p in parts {
                let is_text = p
                    .get("type")
                    .and_then(|t| t.as_str())
                    .map(|t| t == "text" || t == "input_text")
                    .unwrap_or(true);
                if !is_text {
                    continue;
                }
                if let Some(t) = p.get("text").and_then(|t| t.as_str()) {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(t);
                }
            }
            if out.is_empty() { None } else { Some(out) }
        }
        _ => None,
    }
}

/// Parse a non-streaming completion response (OpenAI chat or Anthropic
/// messages). Returns (assistant_text, tokens_in, tokens_out).
pub fn parse_response_json(v: &Value) -> (String, Option<u64>, Option<u64>) {
    let mut text = String::new();
    // OpenAI: choices[].message.content
    if let Some(choices) = v.get("choices").and_then(|c| c.as_array()) {
        for ch in choices {
            if let Some(c) = ch
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
            {
                text.push_str(c);
            }
        }
    }
    // Anthropic: content[] text blocks
    if text.is_empty() {
        if let Some(content) = v.get("content").and_then(|c| c.as_array()) {
            for block in content {
                if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                    if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                        text.push_str(t);
                    }
                }
            }
        }
    }
    let usage = v.get("usage");
    let tin = usage
        .and_then(|u| u.get("prompt_tokens").or_else(|| u.get("input_tokens")))
        .and_then(|x| x.as_u64());
    let tout = usage
        .and_then(|u| {
            u.get("completion_tokens")
                .or_else(|| u.get("output_tokens"))
        })
        .and_then(|x| x.as_u64());
    (text, tin, tout)
}

/// Assemble assistant text and usage from a captured SSE stream
/// (handles both OpenAI `delta.content` chunks and Anthropic
/// `content_block_delta` / `message_delta` events).
pub fn parse_sse(body: &str) -> (String, Option<u64>, Option<u64>) {
    let mut text = String::new();
    let mut tin: Option<u64> = None;
    let mut tout: Option<u64> = None;
    for line in body.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        // OpenAI streaming: choices[].delta.content
        if let Some(choices) = v.get("choices").and_then(|c| c.as_array()) {
            for ch in choices {
                if let Some(d) = ch
                    .get("delta")
                    .and_then(|d| d.get("content"))
                    .and_then(|c| c.as_str())
                {
                    text.push_str(d);
                }
            }
        }
        // Anthropic streaming: content_block_delta -> delta.text
        if let Some(t) = v
            .get("delta")
            .and_then(|d| d.get("text"))
            .and_then(|t| t.as_str())
        {
            text.push_str(t);
        }
        // Usage, wherever it appears (final OpenAI chunk / Anthropic message_delta)
        if let Some(u) = v.get("usage") {
            if let Some(x) = u
                .get("prompt_tokens")
                .or_else(|| u.get("input_tokens"))
                .and_then(|x| x.as_u64())
            {
                tin = Some(x);
            }
            if let Some(x) = u
                .get("completion_tokens")
                .or_else(|| u.get("output_tokens"))
                .and_then(|x| x.as_u64())
            {
                tout = Some(x);
            }
        }
        // Anthropic message_start carries input usage
        if let Some(x) = v
            .get("message")
            .and_then(|m| m.get("usage"))
            .and_then(|u| u.get("input_tokens"))
            .and_then(|x| x.as_u64())
        {
            tin = Some(x);
        }
    }
    (text, tin, tout)
}

// ---------------------------------------------------------------------------
// Cost estimation (best effort)
// ---------------------------------------------------------------------------

/// USD per 1M tokens (input, output). Prefix-matched against the model id.
/// Best-effort defaults; exact billing always belongs to the provider.
/// More specific prefixes must come before less specific ones.
const PRICES_PER_MTOK: &[(&str, f64, f64)] = &[
    ("gpt-4o-mini", 0.15, 0.60),
    ("gpt-4o", 2.50, 10.00),
    ("gpt-4.1-mini", 0.40, 1.60),
    ("gpt-4.1-nano", 0.10, 0.40),
    ("gpt-4.1", 2.00, 8.00),
    ("o4-mini", 1.10, 4.40),
    ("o3", 2.00, 8.00),
    ("claude-3-5-haiku", 0.80, 4.00),
    ("claude-3-5-sonnet", 3.00, 15.00),
    ("claude-haiku", 1.00, 5.00),
    ("claude-sonnet", 3.00, 15.00),
    ("claude-opus", 15.00, 75.00),
];

pub fn estimate_cost(model: Option<&str>, tin: Option<u64>, tout: Option<u64>) -> Option<f64> {
    let model = model?;
    let (pin, pout) = PRICES_PER_MTOK
        .iter()
        .find(|(prefix, _, _)| model.starts_with(prefix) || model.contains(prefix))
        .map(|(_, i, o)| (*i, *o))?;
    if tin.is_none() && tout.is_none() {
        return None;
    }
    let tin = tin.unwrap_or(0) as f64;
    let tout = tout.unwrap_or(0) as f64;
    Some((tin * pin + tout * pout) / 1_000_000.0)
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ex(ts_ms: u64, text: &str) -> Exchange {
        Exchange {
            ts_ms,
            agent: None,
            model: Some("gpt-4o".into()),
            prompt: Some("fix the bug".into()),
            response_text: text.into(),
            tokens_in: Some(100),
            tokens_out: Some(50),
            cost_usd: None,
        }
    }

    #[test]
    fn correlate_finds_content_match() {
        let added = vec![
            "fn refresh_token(user: &User) -> Result<Token> {".to_string(),
            "    rotate_every(Duration::hours(24))".to_string(),
            "}".to_string(), // insignificant, filtered out
        ];
        let exchanges = vec![
            ex(1_000, "unrelated chatter about the weather"),
            ex(
                2_000,
                "Here is the fix:\n```rust\nfn refresh_token(user: &User) -> Result<Token> {\n    rotate_every(Duration::hours(24))\n}\n```",
            ),
        ];
        let c = correlate(&added, &exchanges).expect("must correlate");
        assert_eq!(c.exchange.ts_ms, 2_000);
        assert_eq!(c.matched, 2);
        assert_eq!(c.considered, 2);
        assert!((c.score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn correlate_rejects_weak_matches() {
        let added = vec![
            "let alpha = compute_alpha(input);".to_string(),
            "let beta = compute_beta(input);".to_string(),
            "let gamma = compute_gamma(input);".to_string(),
            "let delta = compute_delta(input);".to_string(),
            "let epsilon = compute_epsilon(input);".to_string(),
        ];
        // Only 1/5 lines present -> score 0.2 < 0.25 threshold.
        let exchanges = vec![ex(1_000, "let alpha = compute_alpha(input);")];
        assert!(correlate(&added, &exchanges).is_none());
    }

    #[test]
    fn correlate_ignores_trivial_lines() {
        let added = vec!["}".to_string(), "{".to_string(), "  ".to_string()];
        let exchanges = vec![ex(1_000, "} { ")];
        assert!(correlate(&added, &exchanges).is_none());
    }

    #[test]
    fn extract_prompt_openai_string_content() {
        let body = json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "Add JWT refresh logic"},
                {"role": "assistant", "content": "ok"},
                {"role": "user", "content": "rotate every 24h"}
            ]
        });
        assert_eq!(extract_prompt(&body).as_deref(), Some("rotate every 24h"));
    }

    #[test]
    fn extract_prompt_anthropic_block_content() {
        let body = json!({
            "model": "claude-sonnet-4",
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "Fix the failing tests"},
                    {"type": "image", "source": {}}
                ]}
            ]
        });
        assert_eq!(
            extract_prompt(&body).as_deref(),
            Some("Fix the failing tests")
        );
    }

    #[test]
    fn parse_response_json_openai() {
        let v = json!({
            "choices": [{"message": {"role": "assistant", "content": "hello world"}}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 4}
        });
        let (text, tin, tout) = parse_response_json(&v);
        assert_eq!(text, "hello world");
        assert_eq!(tin, Some(10));
        assert_eq!(tout, Some(4));
    }

    #[test]
    fn parse_response_json_anthropic() {
        let v = json!({
            "content": [{"type": "text", "text": "ciao"}],
            "usage": {"input_tokens": 7, "output_tokens": 2}
        });
        let (text, tin, tout) = parse_response_json(&v);
        assert_eq!(text, "ciao");
        assert_eq!(tin, Some(7));
        assert_eq!(tout, Some(2));
    }

    #[test]
    fn parse_sse_openai_stream() {
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n\
                    data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n\
                    data: {\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2}}\n\n\
                    data: [DONE]\n";
        let (text, tin, tout) = parse_sse(body);
        assert_eq!(text, "hello");
        assert_eq!(tin, Some(5));
        assert_eq!(tout, Some(2));
    }

    #[test]
    fn parse_sse_anthropic_stream() {
        let body = "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":12}}}\n\n\
                    data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"ci\"}}\n\n\
                    data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"ao\"}}\n\n\
                    data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}\n";
        let (text, tin, tout) = parse_sse(body);
        assert_eq!(text, "ciao");
        assert_eq!(tin, Some(12));
        assert_eq!(tout, Some(2));
    }

    #[test]
    fn estimate_cost_prefix_match() {
        let c = estimate_cost(Some("gpt-4o-2024-11-20"), Some(1_000_000), Some(1_000_000));
        assert!((c.unwrap() - 12.50).abs() < 1e-9);
        assert!(estimate_cost(Some("unknown-model"), Some(10), Some(10)).is_none());
        assert!(estimate_cost(None, Some(10), Some(10)).is_none());
    }
}
