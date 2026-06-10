use anyhow::{Result, anyhow};
use colored::Colorize;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tiny_http::{Header, Method, Response, Server, StatusCode};

use crate::capture::{
    Exchange, append_jsonl, estimate_cost, exchanges_path, extract_prompt, now_ms,
    parse_response_json, parse_sse,
};
use crate::cli::ProxyArgs;
use crate::repo::Repo;

/// `re proxy` — the heart of the capture layer.
///
/// A local, single-binary LLM proxy. Point any agent at it
/// (`OPENAI_BASE_URL` / `ANTHROPIC_BASE_URL`) and every prompt, completion,
/// token count and dollar flows through Causari on its way to the provider.
/// Bytes are streamed to the client in real time (tee capture), so streaming
/// agents feel no difference.
///
/// Captured exchanges land in `.causari/capture/exchanges.jsonl`, where
/// `re watch` joins them with filesystem changes by *content*: the lines that
/// appear in your files are searched inside the completions that preceded
/// them. That join is what turns "12 files changed" into "12 files changed
/// because this prompt asked this model, and it cost $0.14".
pub fn run(args: ProxyArgs) -> Result<()> {
    let repo = Arc::new(Repo::discover()?);
    let port = args.port.unwrap_or(4242);
    let cfg = Arc::new(ProxyConfig {
        openai: args
            .openai_upstream
            .unwrap_or_else(|| "https://api.openai.com".to_string()),
        anthropic: args
            .anthropic_upstream
            .unwrap_or_else(|| "https://api.anthropic.com".to_string()),
    });

    let server = Server::http(("127.0.0.1", port))
        .map_err(|e| anyhow!("cannot bind 127.0.0.1:{}: {}", port, e))?;

    println!(
        "{} LLM capture proxy listening on {}",
        "causari:".green().bold(),
        format!("http://127.0.0.1:{}", port).cyan()
    );
    println!();
    println!("  Point your agent at it:");
    println!(
        "    {}  {}",
        "OPENAI_BASE_URL".bright_black(),
        format!("http://127.0.0.1:{}/openai/v1", port).bright_white()
    );
    println!(
        "    {}  {}",
        "ANTHROPIC_BASE_URL".bright_black(),
        format!("http://127.0.0.1:{}/anthropic", port).bright_white()
    );
    println!();
    println!(
        "  Captures to {} — run {} in another terminal to join captures with file changes.",
        ".causari/capture/exchanges.jsonl".bright_black(),
        "re watch".cyan()
    );
    println!("  Press Ctrl-C to stop.");
    println!();

    for request in server.incoming_requests() {
        let cfg = Arc::clone(&cfg);
        let repo = Arc::clone(&repo);
        std::thread::spawn(move || {
            if let Err(e) = handle(request, &cfg, &repo) {
                eprintln!("{} {}", "proxy error:".red(), e);
            }
        });
    }
    Ok(())
}

struct ProxyConfig {
    openai: String,
    anthropic: String,
}

/// Map an incoming path to (upstream_base, upstream_path).
/// Explicit prefixes win; bare Anthropic/OpenAI paths fall through for
/// drop-in compatibility with clients that only allow a host override.
fn route(url: &str, cfg: &ProxyConfig) -> (String, String) {
    if let Some(rest) = url.strip_prefix("/anthropic") {
        (cfg.anthropic.clone(), rest.to_string())
    } else if let Some(rest) = url.strip_prefix("/openai") {
        (cfg.openai.clone(), rest.to_string())
    } else if url.starts_with("/v1/messages") {
        (cfg.anthropic.clone(), url.to_string())
    } else {
        (cfg.openai.clone(), url.to_string())
    }
}

/// Endpoints whose traffic is a model completion worth capturing.
fn is_completion_path(path: &str) -> bool {
    path.contains("/chat/completions") || path.contains("/messages") || path.contains("/responses")
}

/// A reader that copies every byte it serves into a shared buffer.
/// This is what lets the proxy stream upstream bytes to the client in real
/// time while still owning a full copy for parsing afterwards.
struct Tee<R: Read> {
    inner: R,
    buf: Arc<Mutex<Vec<u8>>>,
}

impl<R: Read> Read for Tee<R> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(out)?;
        if n > 0 {
            if let Ok(mut b) = self.buf.lock() {
                b.extend_from_slice(&out[..n]);
            }
        }
        Ok(n)
    }
}

const FORWARDED_HEADERS: &[&str] = &[
    "authorization",
    "x-api-key",
    "anthropic-version",
    "anthropic-beta",
    "openai-beta",
    "openai-organization",
    "openai-project",
    "content-type",
    "accept",
    "user-agent",
];

fn handle(mut request: tiny_http::Request, cfg: &ProxyConfig, repo: &Repo) -> Result<()> {
    let url = request.url().to_string();
    let method = request.method().clone();
    let (upstream_base, upstream_path) = route(&url, cfg);
    let full_url = format!("{}{}", upstream_base, upstream_path);

    let mut body = Vec::new();
    request.as_reader().read_to_end(&mut body)?;

    // Request-side metadata (model, prompt, agent identity).
    let body_json: Option<serde_json::Value> = serde_json::from_slice(&body).ok();
    let model = body_json
        .as_ref()
        .and_then(|v| v.get("model"))
        .and_then(|m| m.as_str())
        .map(String::from);
    let prompt = body_json.as_ref().and_then(extract_prompt);
    let user_agent = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("user-agent"))
        .map(|h| h.value.as_str().to_string());

    // Forward upstream. No overall timeout: SSE streams can run for minutes.
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .build();
    let mut req = agent.request(method.as_str(), &full_url);
    for name in FORWARDED_HEADERS {
        if let Some(h) = request.headers().iter().find(|h| h.field.equiv(name)) {
            req = req.set(name, h.value.as_str());
        }
    }
    let upstream = if method == Method::Get {
        req.call()
    } else {
        req.send_bytes(&body)
    };
    let upstream = match upstream {
        Ok(r) => r,
        // Non-2xx still has a body the client needs to see (error details).
        Err(ureq::Error::Status(_, r)) => r,
        Err(e) => {
            let resp = Response::from_string(format!("causari proxy: upstream unreachable: {}", e))
                .with_status_code(502);
            let _ = request.respond(resp);
            return Ok(());
        }
    };

    let status = upstream.status();
    let content_type = upstream
        .header("content-type")
        .unwrap_or("application/octet-stream")
        .to_string();

    // Tee-stream the response: client gets bytes live, we keep a copy.
    let captured = Arc::new(Mutex::new(Vec::new()));
    let tee = Tee {
        inner: upstream.into_reader(),
        buf: Arc::clone(&captured),
    };
    let headers = vec![
        Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes())
            .map_err(|_| anyhow!("invalid content-type header"))?,
    ];
    let response = Response::new(StatusCode(status), headers, tee, None, None);
    request.respond(response)?;

    // Response fully streamed — now parse the copy and write the exchange.
    if !is_completion_path(&upstream_path) || status >= 400 {
        return Ok(());
    }
    let bytes = captured
        .lock()
        .map_err(|_| anyhow!("capture buffer poisoned"))?
        .clone();
    let (text, tokens_in, tokens_out) = if content_type.contains("event-stream") {
        parse_sse(&String::from_utf8_lossy(&bytes))
    } else {
        match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(v) => parse_response_json(&v),
            Err(_) => (String::new(), None, None),
        }
    };
    let cost_usd = estimate_cost(model.as_deref(), tokens_in, tokens_out);
    let exchange = Exchange {
        ts_ms: now_ms(),
        agent: user_agent,
        model: model.clone(),
        prompt: prompt.clone(),
        response_text: text,
        tokens_in,
        tokens_out,
        cost_usd,
    };
    append_jsonl(&exchanges_path(repo), &exchange)?;

    let prompt_preview = prompt
        .as_deref()
        .map(|p| {
            let first = p.lines().next().unwrap_or("");
            let mut s: String = first.chars().take(60).collect();
            if first.chars().count() > 60 {
                s.push('…');
            }
            s
        })
        .unwrap_or_else(|| "(no prompt)".to_string());
    println!(
        "  {} {}  {}{}  {}",
        "•".green(),
        model.as_deref().unwrap_or("unknown-model").cyan(),
        format_tokens(tokens_in, tokens_out).bright_black(),
        cost_usd
            .map(|c| format!("  ${:.4}", c))
            .unwrap_or_default()
            .bright_black(),
        format!("\"{}\"", prompt_preview).italic()
    );
    Ok(())
}

fn format_tokens(tin: Option<u64>, tout: Option<u64>) -> String {
    match (tin, tout) {
        (Some(i), Some(o)) => format!("{}→{} tok", i, o),
        (Some(i), None) => format!("{} tok in", i),
        (None, Some(o)) => format!("{} tok out", o),
        (None, None) => "tokens n/a".to_string(),
    }
}
