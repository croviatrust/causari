use anyhow::{Context, Result, anyhow};
use colored::Colorize;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tiny_http::{Header, Method, Response, Server, StatusCode};

use crate::capture::{
    Exchange, ParsedResponse, append_jsonl, estimate_cost, exchanges_path, extract_prompt, now_ms,
    parse_response_json, parse_sse,
};
use crate::cli::ProxyArgs;
use crate::pnx_reach::Policy;
use crate::pnx_run::{self, ReachConfig, Run};
use crate::repo::Repo;
use crate::seal::{SealGenerator, SealIssuer, SealSubject};

/// `re proxy` — the heart of the capture layer.
///
/// A local, single-binary LLM proxy. Point any agent at it
/// (`OPENAI_BASE_URL` / `ANTHROPIC_BASE_URL`) and every prompt, completion,
/// token count and dollar flows through Causari on its way to the provider.
/// Only `POST`s to `/chat/completions`, `/messages` and `/responses` are
/// captured; everything else is relayed untouched. Response bodies are
/// tee-copied while being relayed. Note that `tiny_http` frames chunked
/// bodies through an 8 KB buffer, so streamed tokens reach the client in
/// 8 KB bursts (short answers arrive whole at completion) — not token by
/// token.
///
/// Captured exchanges land in `.causari/capture/exchanges.jsonl`. The
/// recorded completion covers text *and* tool-call payloads (OpenAI chat
/// `tool_calls`, Anthropic `tool_use`, the Responses API), which is where
/// coding agents put the code they write. `re watch` joins exchanges with
/// filesystem changes by *content*: the lines that appear in your files are
/// searched inside the completions that preceded them. That join is what
/// turns "12 files changed" into "12 files changed because this prompt asked
/// this model, and it cost $0.14". The Claude Code hook does the same join
/// from its side to pick up model and cost.
///
/// With `--pnx` the proxy is also a PNX egress witness (TACET profile
/// `crovia.pnx.v1`): every request body is fingerprinted and persisted
/// *before* it is forwarded, and Ctrl-C closes the run with a signed sheet.
/// A body the witness cannot record is not forwarded: the sheet's claim is
/// "everything that left through here is in the map", and a hole in the map
/// would turn a `present` into an `absent`. The sheet also carries the reach
/// record (PNX §4a): every upstream `host:port` the proxy forwarded to, with
/// its outcome under `--pnx-policy` — refused destinations are answered 403
/// and recorded as `blocked`, never forwarded.
pub fn run(args: ProxyArgs) -> Result<()> {
    let repo = Arc::new(Repo::discover()?);
    let port = args.port.unwrap_or(4242);
    let sealer = if args.seal {
        let issuer = SealIssuer::load_or_create(&repo, args.seal_issuer.clone())?;
        println!(
            "{} Crovia Seal issuer active — pubkey {}",
            "causari:".green().bold(),
            issuer.pubkey_hex().bright_white()
        );
        Some(Mutex::new(issuer))
    } else {
        None
    };
    let witness = if args.pnx {
        let policy = args.pnx_policy.as_deref().map(Policy::load).transpose()?;
        let reach = ReachConfig {
            capture: "proxy-http".to_string(),
            mode: match (&policy, args.pnx_reach_mode.as_deref()) {
                (None, _) => "observe",
                (Some(_), Some(m)) => m,
                (Some(_), None) => "enforce",
            }
            .to_string(),
            disclosure: if args.pnx_reach_salted {
                "salted"
            } else {
                "clear"
            }
            .to_string(),
            policy,
        };
        Some(open_witness(&repo, args.pnx_run_id.as_deref(), reach)?)
    } else {
        None
    };
    let cfg = Arc::new(ProxyConfig {
        openai: args
            .openai_upstream
            .unwrap_or_else(|| "https://api.openai.com".to_string()),
        anthropic: args
            .anthropic_upstream
            .unwrap_or_else(|| "https://api.anthropic.com".to_string()),
        sealer,
        witness,
    });

    let server = Server::http(("127.0.0.1", port))
        .map_err(|e| anyhow!("cannot bind 127.0.0.1:{}: {}", port, e))?;

    if cfg.witness.is_some() {
        // Ctrl-C is how a proxy session ends; in witness mode it is also
        // when the sheet gets signed. The handler waits for any body being
        // recorded at that moment, then closes the run and exits.
        let cfg = Arc::clone(&cfg);
        ctrlc::set_handler(move || {
            println!();
            match close_witness(&cfg) {
                Ok(()) => std::process::exit(0),
                Err(e) => {
                    eprintln!("{} {e}", "pnx:".red());
                    std::process::exit(1)
                }
            }
        })
        .map_err(|e| anyhow!("installing the Ctrl-C handler: {e}"))?;
    }

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
    println!(
        "  {} OpenAI chat requests with {} get {} added, so streamed completions carry tokens and cost.",
        "usage requested on streams:".bright_black(),
        "stream:true".bright_black(),
        "stream_options.include_usage".bright_black()
    );
    if let Some(w) = &cfg.witness {
        let run = w.run.lock().map_err(|_| anyhow!("PNX witness poisoned"))?;
        println!(
            "  {} run {} — every request body is fingerprinted ({}; shared substrings of {} bytes \
             or more are always detected) before it is forwarded; Ctrl-C signs the sheet into {}.",
            "PNX witness:".bright_black(),
            run.run_id().cyan(),
            pnx_run::describe_layers(&run.witness).bright_black(),
            crate::pnx::THRESHOLD,
            run.sheet_path().display().to_string().bright_black()
        );
        if let Some(rc) = run.reach_config() {
            println!(
                "  {} every upstream the proxy forwards to goes into the sheet's reach record ({}); {}",
                "PNX reach:".bright_black(),
                rc.disclosure,
                match &rc.policy {
                    Some(p) if rc.mode == "enforce" => format!(
                        "destinations outside the {}-rule policy {} are refused with 403 and recorded as blocked",
                        p.allow.len(),
                        p.hash()
                    ),
                    Some(p) => format!(
                        "everything is relayed; a verifier holding the {}-rule policy {} judges it",
                        p.allow.len(),
                        p.hash()
                    ),
                    None => "no policy: destinations are stated, not judged".to_string(),
                }
                .bright_black()
            );
        }
    }
    println!("  {}", crate::redact::STORAGE_NOTICE.bright_black());
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
    /// When set, every completion also produces a Crovia Seal
    /// (draft-crovia-seal-01): an Ed25519-signed, hash-chained receipt.
    /// Mutex because the chain state (sequence, prev hash) is strictly serial.
    sealer: Option<Mutex<SealIssuer>>,
    /// When set, every outbound request body is fingerprinted into a PNX
    /// run before it is forwarded (`--pnx`).
    witness: Option<PnxWitness>,
}

/// The PNX run being witnessed and the key that will sign its sheet. The
/// key is loaded up front so that closing the run at Ctrl-C cannot fail on
/// key creation.
struct PnxWitness {
    run: Mutex<Run>,
    key: ed25519_dalek::SigningKey,
}

fn open_witness(repo: &Repo, run_id: Option<&str>, reach: ReachConfig) -> Result<PnxWitness> {
    let key = pnx_run::witness_key(repo)?;
    let run_id = match run_id {
        Some(id) => id.to_string(),
        None => pnx_run::new_run_id()?,
    };
    let (mut run, resumed) = Run::open(repo, &run_id, pnx_run::new_salt()?)?;
    run.start_reach(reach)?;
    println!(
        "{} PNX witness {} — {} run {}",
        "causari:".green().bold(),
        pnx_run::witness_id(&key).bright_white(),
        if resumed { "resuming" } else { "opened" },
        run_id.cyan()
    );
    if resumed {
        println!(
            "  {} bodies, {} fingerprints and {} destinations already recorded in this run",
            run.witness.bodies,
            run.witness.map.len(),
            run.reach_destinations()
        );
    }
    Ok(PnxWitness {
        run: Mutex::new(run),
        key,
    })
}

/// Record one outbound body in the PNX run, persisting before returning.
/// Empty bodies (GETs, health checks) carry nothing and are not counted.
fn witness_body(cfg: &ProxyConfig, body: &[u8]) -> Result<()> {
    let Some(w) = &cfg.witness else {
        return Ok(());
    };
    if body.is_empty() {
        return Ok(());
    }
    let mut run = w.run.lock().map_err(|_| anyhow!("PNX witness poisoned"))?;
    run.ingest(body, &pnx_run::now_rfc3339())
        .context("PNX witness could not record the body")?;
    Ok(())
}

/// `host:port` of an upstream base URL, as the reach record names it. The
/// port is the URL's, else the scheme's default.
fn destination(base: &str) -> Option<(String, u16)> {
    let (scheme, rest) = base.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let default_port = match scheme {
        "https" => 443,
        "http" => 80,
        _ => return None,
    };
    let (host, port) = if let Some(h) = authority.strip_prefix('[') {
        let (host, tail) = h.split_once(']')?;
        (
            host,
            tail.strip_prefix(':').map(str::parse).transpose().ok()?,
        )
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) if !p.is_empty() => (h, Some(p.parse().ok()?)),
            _ => (authority, None),
        }
    };
    if host.is_empty() {
        return None;
    }
    Some((host.to_lowercase(), port.unwrap_or(default_port)))
}

/// Ask the run whether `host:port` may be reached. `Some(false)` means the
/// enforced policy refuses it: the attempt is recorded as blocked and the
/// request must not be forwarded.
fn reach_decide(cfg: &ProxyConfig, host: &str, port: u16) -> Result<Option<bool>> {
    let Some(w) = &cfg.witness else {
        return Ok(None);
    };
    let mut run = w.run.lock().map_err(|_| anyhow!("PNX witness poisoned"))?;
    match run.reach_decide(host, port) {
        None => Ok(None),
        Some("blocked") => {
            run.reach_attempt(host, port, &pnx_run::now_rfc3339(), Some("blocked"), 0, 0)
                .context("PNX witness could not record the refused destination")?;
            Ok(Some(false))
        }
        Some(_) => Ok(Some(true)),
    }
}

/// Record a forwarded (or failed) connection in the run's reach record.
fn reach_record(cfg: &ProxyConfig, host: &str, port: u16, outcome: &str, out: u64, inb: u64) {
    let Some(w) = &cfg.witness else {
        return;
    };
    let Ok(mut run) = w.run.lock() else {
        return;
    };
    if run.reach_config().is_none() {
        return;
    }
    if let Err(e) = run.reach_attempt(host, port, &pnx_run::now_rfc3339(), Some(outcome), out, inb)
    {
        eprintln!("{} reach record: {e:#}", "pnx:".red());
    }
}

/// Sign the run sheet and say where it is. Called from the Ctrl-C handler.
fn close_witness(cfg: &ProxyConfig) -> Result<()> {
    let Some(w) = &cfg.witness else {
        return Ok(());
    };
    // A request thread that panicked while holding the lock leaves the map
    // consistent (fingerprints are appended before counters move), so a
    // poisoned lock is still worth signing.
    let mut run = match w.run.lock() {
        Ok(r) => r,
        Err(poisoned) => poisoned.into_inner(),
    };
    let sheet = run.close(&w.key)?;
    if run.witness.map.is_empty() {
        println!(
            "{} PNX run {} saw no bodies; the sheet commits to an empty map",
            "note:".yellow(),
            run.run_id()
        );
    }
    println!(
        "{} PNX run {} closed — {} bodies, {} bytes, {} fingerprints, {} destinations",
        "causari:".green().bold(),
        run.run_id().cyan(),
        run.witness.bodies,
        run.witness.bytes,
        run.witness.map.len(),
        run.reach_destinations()
    );
    if let Some(sm) = sheet.pointer("/reach/summary") {
        println!(
            "  reach   {} connections: {} allowed, {} blocked, {} failed · policy {} {}",
            sm["connections"],
            sm["allowed"],
            sm["blocked"],
            sm["failed"],
            sheet["reach"]["policy"]["kind"].as_str().unwrap_or("?"),
            sheet["reach"]["policy"]["mode"].as_str().unwrap_or("?")
        );
    }
    println!(
        "  root    {}",
        sheet["root"].as_str().unwrap_or("?").bright_white()
    );
    println!(
        "  sheet   {}  {}",
        run.sheet_path().display(),
        "(public: this is what a verifier needs besides the proof)".bright_black()
    );
    println!(
        "  next    {}",
        format!("re pnx prove --run {} --asset LABEL=PATH", run.run_id()).cyan()
    );
    Ok(())
}

/// Generation parameters worth committing into the seal, stringified per
/// CSC-1 (floats are forbidden in signed payloads).
fn seal_params(body: Option<&serde_json::Value>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(obj) = body.and_then(|v| v.as_object()) {
        for key in ["temperature", "top_p", "max_tokens", "max_output_tokens"] {
            if let Some(v) = obj.get(key) {
                let s = match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                out.push((key.to_string(), s));
            }
        }
    }
    out
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

/// Requests whose response is a model completion worth capturing: a POST
/// to a completion endpoint. Substring matching used to record
/// `/v1/messages/count_tokens` (no completion, no usage) and
/// `GET /v1/responses/{id}` (a replay of a completion already captured)
/// as exchanges of their own.
fn is_completion_request(method: &Method, path: &str) -> bool {
    if *method != Method::Post {
        return false;
    }
    let path = endpoint(path);
    ["/chat/completions", "/messages", "/responses"]
        .iter()
        .any(|suffix| path.ends_with(suffix))
}

/// The path without query string, fragment or trailing slash.
fn endpoint(path: &str) -> &str {
    let path = path.split(['?', '#']).next().unwrap_or("");
    path.strip_suffix('/').unwrap_or(path)
}

/// OpenAI reports usage on a stream only when the client asks for it with
/// `stream_options.include_usage`; most agents do not, so every streamed
/// chat completion was captured with no tokens and no cost. Returns the
/// request body with that option set when it applies (a streaming chat
/// completion with a JSON object body), `None` when the body is forwarded
/// untouched. The one extra terminal chunk (empty `choices`, `usage`) is
/// part of the documented protocol and is passed through to the client.
fn with_stream_usage(body: &serde_json::Value, path: &str) -> Option<serde_json::Value> {
    if !endpoint(path).ends_with("/chat/completions") {
        return None;
    }
    let mut v = body.clone();
    let obj = v.as_object_mut()?;
    if obj.get("stream").and_then(serde_json::Value::as_bool) != Some(true) {
        return None;
    }
    let opts = obj
        .entry("stream_options")
        .or_insert_with(|| serde_json::json!({}));
    if !opts.is_object() {
        *opts = serde_json::json!({});
    }
    let opts = opts.as_object_mut()?;
    if opts
        .get("include_usage")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        return None;
    }
    opts.insert("include_usage".to_string(), serde_json::Value::Bool(true));
    Some(v)
}

/// A reader that copies every byte it serves into a shared buffer.
/// This is what lets the proxy stream upstream bytes to the client in real
/// time while still owning a full copy for parsing afterwards.
///
/// `complete` flips when upstream reaches EOF. `tiny_http` swallows the
/// client-side write errors (`BrokenPipe`, `ConnectionReset`) inside
/// `respond`, so a client that hangs up mid-stream is invisible there; the
/// only reliable sign is that the copy stopped before upstream was drained.
struct Tee<R: Read> {
    inner: R,
    buf: Arc<Mutex<Vec<u8>>>,
    complete: Arc<AtomicBool>,
}

impl<R: Read> Read for Tee<R> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(out)?;
        if n == 0 {
            self.complete.store(true, Ordering::SeqCst);
        } else if let Ok(mut b) = self.buf.lock() {
            b.extend_from_slice(&out[..n]);
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

/// Headers that must not be relayed verbatim through a proxy (RFC 9110
/// §7.6.1) plus framing headers the tee stream re-computes.
fn is_hop_by_hop(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "proxy-connection"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "content-length"
            | "content-encoding"
    )
}

fn handle(mut request: tiny_http::Request, cfg: &ProxyConfig, repo: &Repo) -> Result<()> {
    let url = request.url().to_string();
    let method = request.method().clone();
    let (upstream_base, upstream_path) = route(&url, cfg);
    let full_url = format!("{}{}", upstream_base, upstream_path);

    let mut body = Vec::new();
    request.as_reader().read_to_end(&mut body)?;

    // Request-side metadata (model, prompt, agent identity).
    let mut body_json: Option<serde_json::Value> = serde_json::from_slice(&body).ok();
    if let Some(patched) = body_json
        .as_ref()
        .and_then(|v| with_stream_usage(v, &upstream_path))
    {
        body = serde_json::to_vec(&patched)?;
        body_json = Some(patched);
    }
    let user_agent = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("user-agent"))
        .map(|h| h.value.as_str().to_string());

    // PNX reach: where these bytes are about to go. An enforced policy that
    // refuses the destination answers here; nothing is forwarded.
    let dest = destination(&upstream_base);
    if let Some((host, port)) = &dest {
        match reach_decide(cfg, host, *port) {
            Ok(Some(false)) => {
                let resp = Response::from_string(format!(
                    "causari proxy: destination {host}:{port} is outside the PNX egress policy; not forwarded"
                ))
                .with_status_code(403);
                let _ = request.respond(resp);
                return Ok(());
            }
            Ok(_) => {}
            Err(e) => {
                let resp = Response::from_string(format!(
                    "causari proxy: PNX witness could not record the destination; not forwarded: {e:#}"
                ))
                .with_status_code(503);
                let _ = request.respond(resp);
                return Err(e);
            }
        }
    }

    // PNX: the bytes about to leave are committed to the run map first. If
    // that fails the body does not leave; the client sees why.
    if let Err(e) = witness_body(cfg, &body) {
        let resp = Response::from_string(format!(
            "causari proxy: PNX witness could not record the request body; not forwarded: {e:#}"
        ))
        .with_status_code(503);
        let _ = request.respond(resp);
        return Err(e);
    }

    // Forward upstream. No overall timeout: SSE streams can run for minutes.
    // Non-2xx still has a body the client needs to see (error details), so
    // status codes are never turned into errors here.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut req = ureq::http::Request::builder()
        .method(method.as_str())
        .uri(&full_url);
    for name in FORWARDED_HEADERS {
        if let Some(h) = request.headers().iter().find(|h| h.field.equiv(name)) {
            req = req.header(*name, h.value.as_str());
        }
    }
    let upstream = if method == Method::Get {
        req.body(())
            .map_err(|e| anyhow!("invalid upstream request: {}", e))
            .and_then(|r| agent.run(r).map_err(Into::into))
    } else {
        req.body(body.as_slice())
            .map_err(|e| anyhow!("invalid upstream request: {}", e))
            .and_then(|r| agent.run(r).map_err(Into::into))
    };
    let upstream = match upstream {
        Ok(r) => r,
        Err(e) => {
            if let Some((host, port)) = &dest {
                reach_record(cfg, host, *port, "failed", body.len() as u64, 0);
            }
            let resp = Response::from_string(format!("causari proxy: upstream unreachable: {}", e))
                .with_status_code(502);
            let _ = request.respond(resp);
            return Ok(());
        }
    };

    let status = upstream.status().as_u16();
    let content_type = upstream
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    // Forward every end-to-end response header (Retry-After, request ids,
    // x-ratelimit-*, openai-*/anthropic-* metadata). Hop-by-hop and framing
    // headers are dropped: the tee re-frames the body, and ureq may already
    // have decoded the content encoding.
    let mut headers = Vec::new();
    for (name, value) in upstream.headers() {
        if is_hop_by_hop(name.as_str()) {
            continue;
        }
        if let Ok(h) = Header::from_bytes(name.as_str().as_bytes(), value.as_bytes()) {
            headers.push(h);
        }
    }
    if !headers.iter().any(|h| h.field.equiv("content-type")) {
        headers.push(
            Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes())
                .map_err(|_| anyhow!("invalid content-type header"))?,
        );
    }

    // Tee-stream the response: client gets bytes live, we keep a copy.
    let captured = Arc::new(Mutex::new(Vec::new()));
    let complete = Arc::new(AtomicBool::new(false));
    let tee = Tee {
        inner: upstream.into_body().into_reader(),
        buf: Arc::clone(&captured),
        complete: Arc::clone(&complete),
    };
    let response = Response::new(StatusCode(status), headers, tee, None, None);
    // When the client hangs up (or upstream breaks) part way through, the
    // provider has billed the call anyway and the bytes seen so far are
    // still evidence: the exchange is recorded as truncated, not dropped.
    let respond_failed = request.respond(response).is_err();
    let truncated = respond_failed || !complete.load(Ordering::SeqCst);
    if let Some((host, port)) = &dest {
        let relayed = captured.lock().map(|b| b.len() as u64).unwrap_or(0);
        reach_record(cfg, host, *port, "allowed", body.len() as u64, relayed);
    }

    if !is_completion_request(&method, &upstream_path) || status >= 400 {
        return Ok(());
    }
    let bytes = captured
        .lock()
        .map_err(|_| anyhow!("capture buffer poisoned"))?
        .clone();
    let exchange = record_exchange(
        repo,
        cfg,
        Captured {
            request_body: &body,
            request_json: body_json.as_ref(),
            response_bytes: &bytes,
            content_type: &content_type,
            user_agent,
            truncated,
        },
    )?;
    print_exchange(&exchange);
    Ok(())
}

/// One completion as it crossed the wire, ready to be turned into an
/// `Exchange`. `response_bytes` is everything relayed to the client; when
/// `truncated`, that is a prefix of what upstream sent.
struct Captured<'a> {
    request_body: &'a [u8],
    request_json: Option<&'a serde_json::Value>,
    response_bytes: &'a [u8],
    content_type: &'a str,
    user_agent: Option<String>,
    truncated: bool,
}

/// Parse the captured completion, optionally seal it, and append it to
/// `exchanges.jsonl`.
fn record_exchange(repo: &Repo, cfg: &ProxyConfig, c: Captured<'_>) -> Result<Exchange> {
    let requested_model = c
        .request_json
        .and_then(|v| v.get("model"))
        .and_then(|m| m.as_str())
        .map(String::from);
    let prompt = c.request_json.and_then(extract_prompt);

    let parsed = if c.content_type.contains("event-stream") {
        parse_sse(&String::from_utf8_lossy(c.response_bytes))
    } else {
        serde_json::from_slice::<serde_json::Value>(c.response_bytes)
            .map(|v| parse_response_json(&v))
            .unwrap_or_default()
    };
    let ParsedResponse {
        text,
        tokens_in,
        tokens_out,
        model: served_model,
    } = parsed;
    // The response names the model that actually answered (a dated snapshot
    // behind an alias, a fallback): that is what was billed.
    let model = served_model.or(requested_model);
    let cost_usd = estimate_cost(model.as_deref(), tokens_in, tokens_out);

    // Optionally emit a Crovia Seal over the exact wire bytes: the request
    // as sent upstream, the response as returned to the client. The seal
    // commits to hashes only — content never leaves the machine. It is
    // emitted first so the exchange record can carry its id.
    let seal_id = if let Some(sealer) = &cfg.sealer {
        let mut issuer = sealer.lock().map_err(|_| anyhow!("seal issuer poisoned"))?;
        let seal = issuer.emit(
            SealSubject {
                input: c.request_body,
                output: c.response_bytes,
                modality: "text",
            },
            SealGenerator {
                id: model.as_deref().unwrap_or("unknown"),
                version: None,
                params: seal_params(c.request_json),
            },
        )?;
        seal["seal_id"].as_str().map(String::from)
    } else {
        None
    };

    let mut exchange = Exchange {
        id: Some(crate::capture::new_exchange_id()?),
        ts_ms: now_ms(),
        agent: c.user_agent,
        model,
        prompt,
        response_text: text,
        tokens_in,
        tokens_out,
        cost_usd,
        request_sha256: Some(crate::seal::sha256_hex(c.request_body)),
        response_sha256: Some(crate::seal::sha256_hex(c.response_bytes)),
        seal_id,
        truncated: c.truncated,
        redactions: 0,
    };
    // The seal, when one was emitted, covers the wire bytes by hash; the
    // stored text is what a reader sees, and it must not carry a pasted key.
    exchange.redact_secrets();
    append_jsonl(&exchanges_path(repo), &exchange)?;
    Ok(exchange)
}

fn print_exchange(e: &Exchange) {
    let prompt_preview = e
        .prompt
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
        "  {} {}  {}{}  {}{}{}",
        "•".green(),
        e.model.as_deref().unwrap_or("unknown-model").cyan(),
        format_tokens(e.tokens_in, e.tokens_out).bright_black(),
        e.cost_usd
            .map(|c| format!("  ${:.4}", c))
            .unwrap_or_default()
            .bright_black(),
        format!("\"{}\"", prompt_preview).italic(),
        e.seal_id
            .as_deref()
            .map(|id| format!("  🔏 {}", id))
            .unwrap_or_default()
            .bright_black(),
        if e.truncated {
            "  (client disconnected; partial)".yellow()
        } else {
            "".normal()
        }
    );
}

fn format_tokens(tin: Option<u64>, tout: Option<u64>) -> String {
    match (tin, tout) {
        (Some(i), Some(o)) => format!("{}→{} tok", i, o),
        (Some(i), None) => format!("{} tok in", i),
        (None, Some(o)) => format!("{} tok out", o),
        (None, None) => "tokens n/a".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_requests_are_posts_to_completion_endpoints() {
        let post = Method::Post;
        assert!(is_completion_request(&post, "/v1/chat/completions"));
        assert!(is_completion_request(&post, "/v1/messages"));
        assert!(is_completion_request(&post, "/v1/responses"));
        assert!(is_completion_request(&post, "/v1/responses/"));
        assert!(is_completion_request(&post, "/v1/messages?beta=true"));
        assert!(is_completion_request(
            &post,
            "/openai/deployments/gpt-4o/chat/completions?api-version=2024-10-21"
        ));
    }

    #[test]
    fn side_endpoints_and_reads_are_not_completions() {
        let post = Method::Post;
        assert!(!is_completion_request(&post, "/v1/messages/count_tokens"));
        assert!(!is_completion_request(&post, "/v1/messages/batches"));
        assert!(!is_completion_request(
            &post,
            "/v1/responses/resp_123/cancel"
        ));
        assert!(!is_completion_request(&post, "/v1/embeddings"));
        assert!(!is_completion_request(
            &Method::Get,
            "/v1/responses/resp_123"
        ));
        assert!(!is_completion_request(&Method::Get, "/v1/responses"));
        assert!(!is_completion_request(
            &Method::Delete,
            "/v1/responses/resp_123"
        ));
    }

    #[test]
    fn streaming_chat_requests_get_usage_requested() {
        use serde_json::json;
        let path = "/v1/chat/completions";
        let body = json!({"model": "gpt-4o", "stream": true, "messages": []});
        let patched = with_stream_usage(&body, path).expect("patched");
        assert_eq!(patched["stream_options"]["include_usage"], json!(true));
        assert_eq!(patched["model"], json!("gpt-4o"), "rest of the body intact");

        // Existing stream_options are extended, not replaced.
        let body = json!({"stream": true, "stream_options": {"other": 1}});
        let patched = with_stream_usage(&body, path).unwrap();
        assert_eq!(patched["stream_options"]["other"], json!(1));
        assert_eq!(patched["stream_options"]["include_usage"], json!(true));

        // A malformed stream_options is replaced rather than forwarded broken.
        let body = json!({"stream": true, "stream_options": null});
        assert_eq!(
            with_stream_usage(&body, path).unwrap()["stream_options"]["include_usage"],
            json!(true)
        );
    }

    #[test]
    fn non_streaming_other_endpoints_and_explicit_opt_in_are_left_alone() {
        use serde_json::json;
        let path = "/v1/chat/completions";
        assert!(with_stream_usage(&json!({"model": "gpt-4o", "stream": false}), path).is_none());
        assert!(with_stream_usage(&json!({"model": "gpt-4o"}), path).is_none());
        assert!(
            with_stream_usage(
                &json!({"stream": true, "stream_options": {"include_usage": true}}),
                path
            )
            .is_none()
        );
        assert!(with_stream_usage(&json!({"stream": true}), "/v1/responses").is_none());
        assert!(with_stream_usage(&json!({"stream": true}), "/v1/messages").is_none());
        assert!(with_stream_usage(&json!([1, 2]), path).is_none());
    }

    #[test]
    fn tee_reports_completion_only_at_upstream_eof() {
        let upstream: &[u8] = b"data: one\n\ndata: two\n\n";
        let buf = Arc::new(Mutex::new(Vec::new()));
        let complete = Arc::new(AtomicBool::new(false));
        let mut tee = Tee {
            inner: upstream,
            buf: Arc::clone(&buf),
            complete: Arc::clone(&complete),
        };
        // The client-side copy stops after the first read: what a hung-up
        // client looks like from here.
        let mut out = [0u8; 11];
        tee.read_exact(&mut out).unwrap();
        assert_eq!(&out[..], b"data: one\n\n");
        assert!(!complete.load(Ordering::SeqCst));
        assert_eq!(buf.lock().unwrap().as_slice(), b"data: one\n\n");

        // Draining upstream flips the flag and the copy is whole.
        let mut rest = Vec::new();
        tee.read_to_end(&mut rest).unwrap();
        assert!(complete.load(Ordering::SeqCst));
        assert_eq!(buf.lock().unwrap().as_slice(), upstream);
    }

    #[test]
    fn a_stream_cut_by_the_client_is_still_recorded_as_truncated() {
        use serde_json::json;
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        let cfg = ProxyConfig {
            openai: String::new(),
            anthropic: String::new(),
            sealer: None,
            witness: None,
        };
        let request = json!({"model": "gpt-4o", "stream": true,
            "messages": [{"role": "user", "content": "add the helper"}]});
        let request_body = serde_json::to_vec(&request).unwrap();
        // The client hung up after two chunks: no finish_reason, no usage.
        let partial = "data: {\"model\":\"gpt-4o-2024-08-06\",\"choices\":[{\"delta\":{\"content\":\"def helper():\\n\"}}]}\n\n\
                       data: {\"model\":\"gpt-4o-2024-08-06\",\"choices\":[{\"delta\":{\"content\":\"    return 4\"}}]}\n\n\
                       data: {\"model\":\"gpt-4o-2024-08-06\",\"choi";

        let e = record_exchange(
            &repo,
            &cfg,
            Captured {
                request_body: &request_body,
                request_json: Some(&request),
                response_bytes: partial.as_bytes(),
                content_type: "text/event-stream",
                user_agent: Some("aider/0.86".into()),
                truncated: true,
            },
        )
        .unwrap();
        assert!(e.truncated);
        assert_eq!(e.response_text, "def helper():\n    return 4");
        assert_eq!(e.model.as_deref(), Some("gpt-4o-2024-08-06"));
        assert_eq!(e.prompt.as_deref(), Some("add the helper"));
        assert_eq!((e.tokens_in, e.tokens_out, e.cost_usd), (None, None, None));

        // Persisted with the flag, and loadable by the join.
        let raw = std::fs::read_to_string(exchanges_path(&repo)).unwrap();
        assert!(raw.contains("\"truncated\":true"), "{raw}");
        let loaded = crate::capture::load_exchanges_since(&repo, 0).unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(loaded[0].truncated);

        // A complete exchange carries no flag at all, so older readers and
        // legacy lines (no field) mean the same thing: not truncated.
        let full = record_exchange(
            &repo,
            &cfg,
            Captured {
                request_body: &request_body,
                request_json: Some(&request),
                response_bytes: br#"{"model":"gpt-4o-2024-08-06","choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":3,"completion_tokens":1}}"#,
                content_type: "application/json",
                user_agent: None,
                truncated: false,
            },
        )
        .unwrap();
        assert!(!full.truncated);
        let last = std::fs::read_to_string(exchanges_path(&repo)).unwrap();
        assert!(!last.lines().last().unwrap().contains("truncated"));
    }

    #[test]
    fn destinations_are_named_from_the_upstream_url() {
        assert_eq!(
            destination("https://api.openai.com"),
            Some(("api.openai.com".into(), 443))
        );
        assert_eq!(
            destination("http://127.0.0.1:4711/v1"),
            Some(("127.0.0.1".into(), 4711))
        );
        assert_eq!(
            destination("https://User@API.Example.COM:8443/x?y"),
            Some(("api.example.com".into(), 8443))
        );
        assert_eq!(destination("http://[::1]:8080"), Some(("::1".into(), 8080)));
        assert_eq!(destination("ftp://x"), None);
        assert_eq!(destination("nonsense"), None);
    }

    #[test]
    fn witness_mode_records_bodies_before_forwarding_and_signs_on_close() {
        use crate::pnx::{verify_proof, verify_sheet};
        use serde_json::json;
        use std::collections::BTreeMap;
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        let policy = Policy {
            allow: vec!["api.openai.com:443".to_string()],
        };
        let reach = ReachConfig {
            capture: "proxy-http".to_string(),
            policy: Some(policy.clone()),
            mode: "enforce".to_string(),
            disclosure: "clear".to_string(),
        };
        let cfg = ProxyConfig {
            openai: String::new(),
            anthropic: String::new(),
            sealer: None,
            witness: Some(open_witness(&repo, Some("session-1"), reach.clone()).unwrap()),
        };
        // Where the bytes go is decided before they leave: the policy
        // refuses the Anthropic upstream, and the refusal is recorded.
        assert_eq!(
            reach_decide(&cfg, "api.openai.com", 443).unwrap(),
            Some(true)
        );
        assert_eq!(
            reach_decide(&cfg, "api.anthropic.com", 443).unwrap(),
            Some(false)
        );
        reach_record(&cfg, "api.openai.com", 443, "allowed", 900, 4200);
        reach_record(&cfg, "api.openai.com", 443, "allowed", 300, 1000);
        let secret = "sk-live-0123456789abcdef0123456789abcdef0123456789abcdef";
        let leaked = serde_json::to_vec(&json!({"model": "gpt-4o", "messages": [
            {"role": "user", "content": format!("why does this fail? KEY={secret}")}]}))
        .unwrap();
        let clean = serde_json::to_vec(&json!({"model": "gpt-4o", "messages": [
            {"role": "user", "content": "rename the helper and add a docstring please"}]}))
        .unwrap();
        witness_body(&cfg, &leaked).unwrap();
        witness_body(&cfg, &clean).unwrap();
        witness_body(&cfg, b"").unwrap();
        {
            let run = cfg.witness.as_ref().unwrap().run.lock().unwrap();
            assert_eq!(run.witness.bodies, 2, "empty bodies are not egress");
            assert!(!run.is_closed());
        }

        close_witness(&cfg).unwrap();
        let run = Run::load(&repo, "session-1").unwrap();
        let sheet = run.sheet().unwrap();
        assert!(verify_sheet(&sheet).is_empty());
        assert_eq!(sheet["egress"]["bodies"], 2);
        assert_eq!(sheet["reach"]["capture"], "proxy-http");
        assert_eq!(sheet["reach"]["policy"]["hash"], policy.hash());
        assert_eq!(
            sheet["reach"]["summary"],
            json!({
                "destinations": 2, "connections": 3, "allowed": 1, "blocked": 1, "failed": 0
            })
        );
        let dests = sheet["reach"]["destinations"].as_array().unwrap();
        assert_eq!(dests[0]["host"], "api.anthropic.com");
        assert_eq!(dests[0]["outcome"], "blocked");
        assert_eq!(dests[1]["host"], "api.openai.com");
        assert_eq!(
            (
                dests[1]["bytes_out"].as_u64(),
                dests[1]["bytes_in"].as_u64()
            ),
            (Some(1200), Some(5200))
        );
        assert_eq!(
            sheet["witness"]["id"],
            pnx_run::witness_id(&pnx_run::witness_key(&repo).unwrap())
        );

        // The leaked key is found through json-strings-v1; an unrelated one is not.
        let other = "AKIA0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF";
        let mut run = run;
        let proof = run
            .witness
            .prove(
                &sheet,
                &[
                    ("openai".to_string(), secret.as_bytes().to_vec()),
                    ("aws".to_string(), other.as_bytes().to_vec()),
                ],
            )
            .unwrap();
        let supplied: BTreeMap<String, Vec<u8>> = [
            ("openai".to_string(), secret.as_bytes().to_vec()),
            ("aws".to_string(), other.as_bytes().to_vec()),
        ]
        .into_iter()
        .collect();
        let res = verify_proof(&proof, Some(&supplied));
        assert!(res.ok, "{:?}", res.errors);
        let got: BTreeMap<String, String> = res.assets.into_iter().collect();
        assert_eq!(got["openai"], "present");
        assert_eq!(got["aws"], "absent");
        // With the policy document the verifier confirms the record.
        let res = crate::pnx::verify_proof_with(&proof, Some(&supplied), Some(&policy), &[]);
        assert!(res.ok, "{:?}", res.errors);
        assert_eq!(res.reach.unwrap().verdict, "within-policy");
        let wrong = Policy {
            allow: vec!["example.org".to_string()],
        };
        let res = crate::pnx::verify_proof_with(&proof, Some(&supplied), Some(&wrong), &[]);
        assert!(!res.ok && res.errors[0].contains("policy document does not match"));

        // A closed run takes no more bodies: the proxy would refuse to forward.
        assert!(witness_body(&cfg, &clean).is_err());
        // Closing twice is refused as well.
        assert!(close_witness(&cfg).is_err());
    }
}
