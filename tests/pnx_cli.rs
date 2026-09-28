//! `re pnx` and `re proxy --pnx` end to end.
//!
//! Three layers, each skipping cleanly when its precondition is missing:
//!
//! 1. Vectors (always): the proofs the Python reference generated into
//!    `tests/vectors/pnx/witness.json` are verified through the `re pnx
//!    verify` command line, with and without asset bytes, bare and
//!    tampered, checking the `tacet-pnx` exit-code contract (0 / 1 / 2).
//! 2. Proxy (unix): a real `re proxy --pnx` in front of a mock upstream
//!    witnesses two bodies, is stopped with SIGINT, and the run is proved
//!    and verified from the command line.
//! 3. Cross-verification (when `tacet-pnx` is on PATH): the proofs from
//!    layer 2 are verified by `tacet-pnx verify`, and proofs produced by
//!    `tacet-pnx witness` + `prove` — bare, and sealed when `crovia_seal`
//!    is importable — are verified by `re pnx verify`.
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde_json::{Value, json};

fn re(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_re"))
        .args(args)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}

fn code(out: &Output) -> i32 {
    out.status.code().unwrap_or(-1)
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The `--json` report on stdout, or the command's stderr as the failure.
fn json_report(out: &Output) -> Value {
    serde_json::from_str(&stdout(out))
        .unwrap_or_else(|e| panic!("{e}: no JSON report\n{}\n{}", stdout(out), stderr(out)))
}

fn vectors() -> Value {
    serde_json::from_str(include_str!("vectors/pnx/witness.json")).unwrap()
}

fn vec_bytes(v: &Value) -> Vec<u8> {
    match v.get("hex").and_then(Value::as_str) {
        Some(h) => hex::decode(h).unwrap(),
        None => v["text"].as_str().unwrap().as_bytes().to_vec(),
    }
}

/// Write every asset of a vector case to `dir` and return the `--asset`
/// arguments naming them.
fn write_assets(dir: &Path, assets: &Value) -> Vec<String> {
    let mut args = Vec::new();
    for (label, bytes) in assets.as_object().unwrap() {
        let path = dir.join(format!("{label}.bin"));
        fs::write(&path, vec_bytes(bytes)).unwrap();
        args.push("--asset".to_string());
        args.push(format!("{label}={}", path.display()));
    }
    args
}

fn expected_exit(verdict: &str) -> i32 {
    if verdict == "absent" { 0 } else { 1 }
}

// ---------------------------------------------------------------------------
// 1. Vectors from the Python reference through the command line
// ---------------------------------------------------------------------------

#[test]
fn reference_proofs_verify_with_the_tacet_pnx_exit_codes() {
    let doc = vectors();
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    for case in doc["proofs"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let proof_path = dir.join(format!("{name}.proof.json"));
        fs::write(&proof_path, serde_json::to_vec(&case["proof"]).unwrap()).unwrap();
        fs::create_dir_all(dir.join(name)).unwrap();
        let asset_args = write_assets(&dir.join(name), &case["assets"]);
        let refs: Vec<&str> = asset_args.iter().map(String::as_str).collect();

        // With assets: fingerprints recomputed, verdict per asset as the
        // reference stated, exit code by verdict.
        let mut args = vec!["pnx", "verify", proof_path.to_str().unwrap(), "--json"];
        args.extend(refs.iter());
        let out = re(dir, &args);
        let report: Value = serde_json::from_str(&stdout(&out)).expect("JSON report");
        assert_eq!(report["ok"], true, "{name}: {report}");
        assert_eq!(report["verdict"], case["verdict"], "{name}");
        assert_eq!(report["assets"], case["asset_verdicts"], "{name}");
        assert_eq!(report["sealed"], false);
        assert_eq!(
            code(&out),
            expected_exit(case["verdict"].as_str().unwrap()),
            "{name}: exit code"
        );

        // Human output names the run and every asset verdict.
        let mut args = vec!["pnx", "verify", proof_path.to_str().unwrap()];
        args.extend(refs.iter());
        let out = re(dir, &args);
        let text = stdout(&out);
        assert!(text.starts_with("VALID"), "{name}: {text}");
        assert!(text.contains(doc["run_id"].as_str().unwrap()));
        for (label, v) in case["asset_verdicts"].as_object().unwrap() {
            assert!(
                text.contains(&format!("{:<14} {label}", v.as_str().unwrap())),
                "{name}: {text}"
            );
        }

        // Without assets: hash-only mode, a warning, valid all the same;
        // --strict turns the warning into exit 1.
        let out = re(
            dir,
            &["pnx", "verify", proof_path.to_str().unwrap(), "--json"],
        );
        let report: Value = serde_json::from_str(&stdout(&out)).unwrap();
        assert_eq!(report["ok"], true, "{name}");
        assert!(report["warnings"].to_string().contains("not recomputed"));
        assert_eq!(code(&out), expected_exit(case["verdict"].as_str().unwrap()));
        let out = re(
            dir,
            &["pnx", "verify", proof_path.to_str().unwrap(), "--strict"],
        );
        assert_eq!(code(&out), 1, "{name}: --strict on a warning");
    }
}

#[test]
fn tampered_and_malformed_proofs_exit_2() {
    let doc = vectors();
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let case = &doc["proofs"][1];
    assert_eq!(case["name"], "clean");
    let asset_dir = dir.join("assets");
    fs::create_dir_all(&asset_dir).unwrap();
    let asset_args = write_assets(&asset_dir, &case["assets"]);
    let refs: Vec<&str> = asset_args.iter().map(String::as_str).collect();

    // A forged verdict, a signed field changed after signing, and a
    // fingerprint swapped for another key.
    let mut forged = case["proof"].clone();
    forged["assets"][0]["verdict"] = json!("present");
    let mut resigned = case["proof"].clone();
    resigned["sheet"]["egress"]["bodies"] = json!(0);
    let mut swapped = case["proof"].clone();
    swapped["assets"][0]["fingerprints"][0]["key"] = json!("00".repeat(32));
    for (name, proof, needle) in [
        ("forged", forged, "stated verdict"),
        ("resigned", resigned, "signature"),
        ("swapped", swapped, "fingerprint set"),
    ] {
        let path = dir.join(format!("{name}.json"));
        fs::write(&path, serde_json::to_vec(&proof).unwrap()).unwrap();
        let mut args = vec!["pnx", "verify", path.to_str().unwrap(), "--json"];
        args.extend(refs.iter());
        let out = re(dir, &args);
        assert_eq!(code(&out), 2, "{name}");
        let report: Value = serde_json::from_str(&stdout(&out)).unwrap();
        assert_eq!(report["ok"], false, "{name}");
        assert!(
            report["errors"].to_string().contains(needle),
            "{name}: {}",
            report["errors"]
        );
        let out = re(dir, &["pnx", "verify", path.to_str().unwrap()]);
        assert!(stdout(&out).starts_with("INVALID"), "{name}");
    }

    // Wrong asset bytes for a label are refused, not silently accepted.
    let wrong = dir.join("wrong.bin");
    fs::write(
        &wrong,
        b"not the asset that was proven, some other text entirely",
    )
    .unwrap();
    let path = dir.join("clean.json");
    fs::write(&path, serde_json::to_vec(&case["proof"]).unwrap()).unwrap();
    let spec = format!("safe-key={}", wrong.display());
    let mut args = vec!["pnx", "verify", path.to_str().unwrap(), "--asset", &spec];
    // Keep every other asset as proven; only safe-key is swapped.
    for pair in refs.chunks(2) {
        if !pair[1].starts_with("safe-key=") {
            args.extend(pair.iter());
        }
    }
    let out = re(dir, &args);
    assert_eq!(code(&out), 2);
    assert!(stdout(&out).contains("does not match asset_sha256"));

    // Not JSON at all, or a missing file: an error, exit 1 from the CLI.
    fs::write(dir.join("garbage.json"), b"{not json").unwrap();
    let out = re(dir, &["pnx", "verify", "garbage.json"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("parsing"));
}

#[test]
fn a_fresh_repository_has_no_runs_to_prove_against() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    assert!(re(dir, &["init"]).status.success());
    // No run yet: prove says so and list is empty.
    let out = re(dir, &["pnx", "list"]);
    assert!(stdout(&out).contains("no PNX runs yet"));
    let out = re(dir, &["pnx", "prove", "--asset", "x=nope"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("no closed PNX run"));
}

// ---------------------------------------------------------------------------
// 2. A real proxy session witnessed, proved and verified
// ---------------------------------------------------------------------------

#[cfg(unix)]
mod proxy {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::time::{Duration, Instant};

    fn free_port() -> u16 {
        TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    /// The smallest OpenAI-shaped upstream: answers every POST with one
    /// completion. Runs on its own thread until the test ends.
    fn mock_upstream() -> u16 {
        let server = tiny_http::Server::http(("127.0.0.1", 0)).unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        std::thread::spawn(move || {
            for mut req in server.incoming_requests() {
                let mut body = Vec::new();
                req.as_reader().read_to_end(&mut body).unwrap();
                let reply = json!({"model": "gpt-4o-2024-08-06",
                    "choices": [{"message": {"content": "ok"}}],
                    "usage": {"prompt_tokens": 3, "completion_tokens": 1}})
                .to_string();
                let resp = tiny_http::Response::from_string(reply).with_header(
                    tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                        .unwrap(),
                );
                let _ = req.respond(resp);
            }
        });
        port
    }

    fn wait_for_port(port: u16) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("proxy did not start listening on {port}");
    }

    fn post(port: u16, path: &str, body: &[u8]) -> u16 {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        write!(
            s,
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .unwrap();
        s.write_all(body).unwrap();
        let mut reply = Vec::new();
        let _ = s.read_to_end(&mut reply);
        let head = String::from_utf8_lossy(&reply);
        head.split_whitespace().nth(1).unwrap().parse().unwrap()
    }

    /// Start `re proxy --pnx`, send `bodies` through it, stop it with
    /// SIGINT and return the proxy's output. The run is `run_id`.
    fn witnessed_session(dir: &Path, run_id: &str, bodies: &[Vec<u8>]) -> Output {
        let upstream = mock_upstream();
        let port = free_port();
        let upstream_url = format!("http://127.0.0.1:{upstream}");
        let child = Command::new(env!("CARGO_BIN_EXE_re"))
            .args([
                "proxy",
                "--pnx",
                "--pnx-run-id",
                run_id,
                "--port",
                &port.to_string(),
                "--openai-upstream",
                &upstream_url,
            ])
            .current_dir(dir)
            .env("NO_COLOR", "1")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        wait_for_port(port);
        for body in bodies {
            assert_eq!(post(port, "/openai/v1/chat/completions", body), 200);
        }
        // Ctrl-C: the handler closes the run and exits 0.
        unsafe {
            libc::kill(child.id() as libc::pid_t, libc::SIGINT);
        }
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "proxy exit {:?}\n{}\n{}",
            out.status,
            stdout(&out),
            stderr(&out)
        );
        out
    }

    pub const SECRET: &str =
        "OPENAI_API_KEY=sk-live-0123456789abcdef0123456789abcdef0123456789abcdef\n";
    pub const SAFE: &str =
        "AWS_SECRET=AKIAFEDCBA9876543210FEDCBA9876543210FEDCBA9876543210FEDCBA\n";

    pub fn leaked_body() -> Vec<u8> {
        serde_json::to_vec(&json!({"model": "gpt-4o", "messages": [
            {"role": "user", "content": format!("why does auth fail? here is my .env:\n{SECRET}")}]}))
        .unwrap()
    }

    pub fn clean_body() -> Vec<u8> {
        serde_json::to_vec(&json!({"model": "gpt-4o", "messages": [
            {"role": "user", "content": "rename the helper and add a docstring, nothing else"}]}))
        .unwrap()
    }

    /// A repository with one closed run `session` over a leaked and a
    /// clean body, plus `secret.txt` and `safe.txt` next to it.
    pub fn witnessed_repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        assert!(re(dir, &["init"]).status.success());
        fs::write(dir.join("secret.txt"), SECRET).unwrap();
        fs::write(dir.join("safe.txt"), SAFE).unwrap();
        let out = witnessed_session(dir, "session", &[leaked_body(), clean_body()]);
        let text = stdout(&out);
        assert!(text.contains("PNX witness"), "{text}");
        assert!(text.contains("PNX run session closed — 2 bodies"), "{text}");
        assert!(text.contains("root    sha256:"), "{text}");
        tmp
    }

    #[test]
    fn a_proxy_session_yields_a_signed_sheet_and_verifiable_proofs() {
        let tmp = witnessed_repo();
        let dir = tmp.path();
        let run_dir = dir.join(".causari/pnx/session");
        assert!(run_dir.join("sheet.json").exists());
        assert!(run_dir.join("fingerprints.log").exists());
        let log = fs::read_to_string(run_dir.join("fingerprints.log")).unwrap();
        assert!(!log.contains("sk-live"), "the log holds fingerprints only");

        // list and sheet
        let out = re(dir, &["pnx", "list"]);
        let text = stdout(&out);
        assert!(
            text.contains("session") && text.contains("closed"),
            "{text}"
        );
        assert!(text.contains("2 bodies"), "{text}");
        let out = re(dir, &["pnx", "sheet"]);
        let sheet: Value = serde_json::from_str(&stdout(&out)).unwrap();
        assert_eq!(sheet["profile"], "crovia.pnx.v1");
        assert_eq!(sheet["run_id"], "session");
        assert_eq!(sheet["egress"]["bodies"], 2);
        assert_eq!(sheet["normalization"], json!(["json-strings-v1"]));
        assert!(
            sheet["witness"]["id"]
                .as_str()
                .unwrap()
                .starts_with("urn:crovia:pnx-witness:causari:")
        );

        // prove: the leaked key is present (through json-strings-v1), the
        // other absent; --fail-on-present exits 1, default proof path.
        let out = re(
            dir,
            &[
                "pnx",
                "prove",
                "--asset",
                "openai=secret.txt",
                "--asset",
                "aws=safe.txt",
                "--fail-on-present",
            ],
        );
        assert_eq!(code(&out), 1, "{}", stderr(&out));
        let err = stderr(&out);
        assert!(err.contains("verdict present"), "{err}");
        assert!(err.contains("present        openai"), "{err}");
        let proof_path = run_dir.join("proof.json");
        let proof: Value = serde_json::from_str(&fs::read_to_string(&proof_path).unwrap()).unwrap();
        assert_eq!(proof["verdict"], "present");
        assert_eq!(proof["sheet"], sheet);
        let raw = fs::read_to_string(&proof_path).unwrap();
        assert!(
            !raw.contains("sk-live") && !raw.contains("AKIA"),
            "no asset bytes in a proof"
        );

        let out = re(
            dir,
            &[
                "pnx",
                "verify",
                proof_path.to_str().unwrap(),
                "--asset",
                "openai=secret.txt",
                "--asset",
                "aws=safe.txt",
                "--json",
            ],
        );
        assert_eq!(code(&out), 1);
        let report: Value = serde_json::from_str(&stdout(&out)).unwrap();
        assert_eq!(report["ok"], true, "{report}");
        assert_eq!(
            report["assets"],
            json!({"openai": "present", "aws": "absent"})
        );

        // A clean proof to stdout, by --assets-dir and --asset-env.
        fs::create_dir_all(dir.join("protected/keys")).unwrap();
        fs::write(dir.join("protected/keys/aws.txt"), SAFE).unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_re"))
            .args([
                "pnx",
                "prove",
                "--run",
                "session",
                "--assets-dir",
                "protected",
                "--asset-env",
                "PNX_TEST_TOKEN",
                "-o",
                "-",
                "--fail-on-present",
            ])
            .env(
                "PNX_TEST_TOKEN",
                "ghp_ZmVkY2JhOTg3NjU0MzIxMGZlZGNiYTk4NzY1NDMyMTBmZWRj",
            )
            .current_dir(dir)
            .output()
            .unwrap();
        assert_eq!(code(&out), 0, "{}", stderr(&out));
        let clean: Value = serde_json::from_str(&stdout(&out)).unwrap();
        assert_eq!(clean["verdict"], "absent");
        let labels: Vec<&str> = clean["assets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["label"].as_str().unwrap())
            .collect();
        assert_eq!(labels, ["keys/aws.txt", "env:PNX_TEST_TOKEN"]);
        fs::write(dir.join("clean.json"), serde_json::to_vec(&clean).unwrap()).unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_re"))
            .args([
                "pnx",
                "verify",
                "clean.json",
                "--assets-dir",
                "protected",
                "--asset-env",
                "PNX_TEST_TOKEN",
            ])
            .env(
                "PNX_TEST_TOKEN",
                "ghp_ZmVkY2JhOTg3NjU0MzIxMGZlZGNiYTk4NzY1NDMyMTBmZWRj",
            )
            .current_dir(dir)
            .output()
            .unwrap();
        assert_eq!(code(&out), 0, "{}", stdout(&out));

        // The run is closed: no second sheet, no more bodies.
        let out = re(dir, &["pnx", "sheet", "--close", "session"]);
        assert!(!out.status.success());
        assert!(stderr(&out).contains("already closed"));
    }

    #[test]
    fn a_run_left_open_is_closed_by_re_pnx_sheet() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        assert!(re(dir, &["init"]).status.success());
        let upstream = mock_upstream();
        let port = free_port();
        let mut child = Command::new(env!("CARGO_BIN_EXE_re"))
            .args([
                "proxy",
                "--pnx",
                "--pnx-run-id",
                "crashed",
                "--port",
                &port.to_string(),
                "--openai-upstream",
                &format!("http://127.0.0.1:{upstream}"),
            ])
            .current_dir(dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        wait_for_port(port);
        assert_eq!(post(port, "/v1/chat/completions", &clean_body()), 200);
        // SIGKILL: no handler runs, the run stays open with its log.
        child.kill().unwrap();
        child.wait().unwrap();

        let out = re(dir, &["pnx", "list"]);
        assert!(stdout(&out).contains("open"), "{}", stdout(&out));
        let out = re(
            dir,
            &["pnx", "prove", "--run", "crashed", "--asset", "x=safe.txt"],
        );
        assert!(stderr(&out).contains("still open"), "{}", stderr(&out));

        let out = re(dir, &["pnx", "sheet", "--close"]);
        assert!(out.status.success(), "{}", stderr(&out));
        let sheet: Value = serde_json::from_str(&stdout(&out)).unwrap();
        assert_eq!(sheet["run_id"], "crashed");
        assert_eq!(sheet["egress"]["bodies"], 1);
        let out = re(dir, &["pnx", "list"]);
        assert!(stdout(&out).contains("closed"));
    }

    #[test]
    fn an_enforced_policy_refuses_outside_destinations_and_the_sheet_says_where_the_run_went() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        assert!(re(dir, &["init"]).status.success());
        fs::write(dir.join("safe.txt"), SAFE).unwrap();
        let upstream = mock_upstream();
        let elsewhere = free_port();
        let policy = json!({"version": "crovia.pnx.policy.v1",
                            "allow": [format!("127.0.0.1:{upstream}")]});
        fs::write(dir.join("policy.json"), policy.to_string()).unwrap();
        fs::write(
            dir.join("other-policy.json"),
            json!({"version": "crovia.pnx.policy.v1", "allow": ["example.org:443"]}).to_string(),
        )
        .unwrap();
        let port = free_port();
        let child = Command::new(env!("CARGO_BIN_EXE_re"))
            .args([
                "proxy",
                "--pnx",
                "--pnx-run-id",
                "policed",
                "--pnx-policy",
                "policy.json",
                "--port",
                &port.to_string(),
                "--openai-upstream",
                &format!("http://127.0.0.1:{upstream}"),
                "--anthropic-upstream",
                &format!("http://127.0.0.1:{elsewhere}"),
            ])
            .current_dir(dir)
            .env("NO_COLOR", "1")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        wait_for_port(port);
        assert_eq!(
            post(port, "/openai/v1/chat/completions", &clean_body()),
            200
        );
        assert_eq!(
            post(port, "/openai/v1/chat/completions", &clean_body()),
            200
        );
        // Outside the policy: refused before anything is forwarded (nothing
        // listens there, and the client still gets a 403, not a 502).
        assert_eq!(post(port, "/anthropic/v1/messages", &clean_body()), 403);
        unsafe {
            libc::kill(child.id() as libc::pid_t, libc::SIGINT);
        }
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
        let text = stdout(&out);
        assert!(text.contains("PNX reach:"), "{text}");
        assert!(text.contains("2 destinations"), "{text}");
        assert!(text.contains("1 allowed, 1 blocked"), "{text}");

        let run_dir = dir.join(".causari/pnx/policed");
        let reach_log = fs::read_to_string(run_dir.join("reach.jsonl")).unwrap();
        assert_eq!(reach_log.lines().count(), 3, "{reach_log}");
        let sheet: Value =
            serde_json::from_str(&fs::read_to_string(run_dir.join("sheet.json")).unwrap()).unwrap();
        let reach = &sheet["reach"];
        assert_eq!(reach["version"], "crovia.pnx.reach.v1");
        assert_eq!(reach["capture"], "proxy-http");
        assert_eq!(reach["policy"]["kind"], "allowlist");
        assert_eq!(reach["policy"]["mode"], "enforce");
        assert_eq!(reach["policy"]["rules"], 1);
        let dests = reach["destinations"].as_array().unwrap();
        assert_eq!(dests.len(), 2);
        let blocked = dests.iter().find(|d| d["outcome"] == "blocked").unwrap();
        assert_eq!(blocked["port"], elsewhere);
        assert_eq!(blocked["bytes_out"], 0);
        let allowed = dests.iter().find(|d| d["outcome"] == "allowed").unwrap();
        assert_eq!(allowed["port"], upstream);
        assert_eq!(allowed["connections"], 2);
        assert!(allowed["bytes_in"].as_u64().unwrap() > 0);

        // Prove, then verify with the policy: within-policy, exit 0.
        let out = re(
            dir,
            &[
                "pnx",
                "prove",
                "--run",
                "policed",
                "--asset",
                "aws=safe.txt",
            ],
        );
        assert!(out.status.success(), "{}", stderr(&out));
        let proof = run_dir.join("proof.json");
        let proof = proof.to_str().unwrap();
        let out = re(
            dir,
            &[
                "pnx",
                "verify",
                proof,
                "--asset",
                "aws=safe.txt",
                "--policy",
                "policy.json",
                "--json",
            ],
        );
        let rep = json_report(&out);
        assert_eq!(code(&out), 0, "{rep}");
        assert_eq!(rep["reach"]["verdict"], "within-policy", "{rep}");
        assert_eq!(rep["reach"]["outside"], json!([]));
        let out = re(
            dir,
            &[
                "pnx",
                "verify",
                proof,
                "--asset",
                "aws=safe.txt",
                "--policy",
                "policy.json",
            ],
        );
        let text = stdout(&out);
        assert!(text.contains("reach  within-policy"), "{text}");
        assert!(text.contains("blocked"), "{text}");

        // Without the document the record alone supports the verdict, with a warning.
        let out = re(
            dir,
            &["pnx", "verify", proof, "--asset", "aws=safe.txt", "--json"],
        );
        let rep = json_report(&out);
        assert_eq!(code(&out), 0, "{rep}");
        assert_eq!(rep["reach"]["verdict"], "within-policy");
        assert!(
            rep["warnings"]
                .to_string()
                .contains("policy document not supplied"),
            "{rep}"
        );

        // Another policy document does not match the bound hash: invalid.
        let out = re(
            dir,
            &[
                "pnx",
                "verify",
                proof,
                "--asset",
                "aws=safe.txt",
                "--policy",
                "other-policy.json",
                "--json",
            ],
        );
        let rep = json_report(&out);
        assert_eq!(code(&out), 2, "{rep}");
        assert!(
            rep["errors"]
                .to_string()
                .contains("policy document does not match"),
            "{rep}"
        );
    }
}

// ---------------------------------------------------------------------------
// 3. Cross-verification with the Python `tacet-pnx` command line
// ---------------------------------------------------------------------------

fn tacet_pnx() -> Option<PathBuf> {
    let candidates: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join("tacet-pnx"))
                .collect()
        })
        .unwrap_or_default();
    let found = candidates.into_iter().find(|p| p.is_file());
    if found.is_none() {
        eprintln!(
            "skipping tacet-pnx cross-check: `tacet-pnx` not on PATH (pip install crovia-tacet)"
        );
    }
    found
}

fn tacet(bin: &Path, dir: &Path, args: &[&str]) -> Output {
    Command::new(bin)
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
}

#[cfg(unix)]
#[test]
fn tacet_pnx_verifies_proofs_from_re_pnx_prove() {
    let Some(bin) = tacet_pnx() else { return };
    let tmp = proxy::witnessed_repo();
    let dir = tmp.path();
    let out = re(
        dir,
        &[
            "pnx",
            "prove",
            "--asset",
            "openai=secret.txt",
            "--asset",
            "aws=safe.txt",
            "-o",
            "exposed.json",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let out = re(
        dir,
        &[
            "pnx",
            "prove",
            "--asset",
            "aws=safe.txt",
            "-o",
            "clean.json",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));

    // Same verdicts, same exit codes, from the reference verifier.
    let out = tacet(
        &bin,
        dir,
        &[
            "verify",
            "exposed.json",
            "--asset",
            "openai=secret.txt",
            "--asset",
            "aws=safe.txt",
            "--json",
        ],
    );
    let report: Value = json_report(&out);
    assert_eq!(report["ok"], true, "{report}");
    assert_eq!(report["verdict"], "present");
    assert_eq!(
        report["assets"],
        json!({"openai": "present", "aws": "absent"})
    );
    assert_eq!(code(&out), 1);

    let out = tacet(
        &bin,
        dir,
        &["verify", "clean.json", "--asset", "aws=safe.txt"],
    );
    assert_eq!(code(&out), 0, "{}\n{}", stdout(&out), stderr(&out));
    assert!(stdout(&out).starts_with("VALID"));
    let out = tacet(&bin, dir, &["verify", "clean.json", "--strict"]);
    assert_eq!(code(&out), 1, "hash-only mode is a warning under --strict");

    // A Rust proof tampered with fails under the reference too.
    let mut bad: Value =
        serde_json::from_str(&fs::read_to_string(dir.join("clean.json")).unwrap()).unwrap();
    bad["sheet"]["egress"]["bytes"] = json!(1);
    fs::write(dir.join("bad.json"), serde_json::to_vec(&bad).unwrap()).unwrap();
    let out = tacet(
        &bin,
        dir,
        &["verify", "bad.json", "--asset", "aws=safe.txt"],
    );
    assert_eq!(code(&out), 2);
}

#[test]
fn re_pnx_verifies_proofs_from_tacet_pnx() {
    let Some(bin) = tacet_pnx() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let secret = "DATABASE_URL=postgres://svc:hunter2hunter2hunter2@db.internal:5432/prod\n";
    let safe = "STRIPE_KEY=sk_test_51H0000000000000000000000000000000000000000\n";
    fs::write(dir.join("secret.txt"), secret).unwrap();
    fs::write(dir.join("safe.txt"), safe).unwrap();
    fs::create_dir_all(dir.join("egress")).unwrap();
    // One body per file, and one capture log line: both forms tacet-pnx reads.
    fs::write(
        dir.join("egress/1.json"),
        serde_json::to_vec(&json!({"messages": [{"role": "user",
            "content": format!("the connection string is\n{secret}")}]}))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        dir.join("egress/log.jsonl"),
        format!(
            "{}\n",
            json!({"at": "2026-09-20T10:00:00Z",
                "body": json!({"messages": [{"content": "rename the helper please"}]}).to_string()})
        ),
    )
    .unwrap();

    let out = tacet(
        &bin,
        dir,
        &["keygen", "--id", "urn:test:witness", "--out", "w.key.json"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let out = tacet(
        &bin,
        dir,
        &[
            "witness",
            "--run-id",
            "py-run",
            "--key",
            "w.key.json",
            "egress",
            "--sheet",
            "sheet.json",
            "--state",
            "state.json",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let out = tacet(
        &bin,
        dir,
        &[
            "prove",
            "--state",
            "state.json",
            "--sheet",
            "sheet.json",
            "--asset",
            "db=secret.txt",
            "--asset",
            "stripe=safe.txt",
            "--out",
            "proof.json",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));

    // The Python proof through the Rust verifier: with assets, hash-only.
    let out = re(
        dir,
        &[
            "pnx",
            "verify",
            "proof.json",
            "--asset",
            "db=secret.txt",
            "--asset",
            "stripe=safe.txt",
            "--json",
        ],
    );
    let report: Value = json_report(&out);
    assert_eq!(report["ok"], true, "{report}");
    assert_eq!(report["verdict"], "present");
    assert_eq!(
        report["assets"],
        json!({"db": "present", "stripe": "absent"})
    );
    assert_eq!(code(&out), 1);
    let out = re(dir, &["pnx", "verify", "proof.json"]);
    assert_eq!(code(&out), 1);
    assert!(stdout(&out).contains("witness urn:test:witness"));

    // `re pnx sheet` reads a sheet.json written by the reference too, as
    // a path; proving from it needs the Rust run state, which there is not.
    let out = re(dir, &["pnx", "sheet", "sheet.json"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("no PNX run at"), "{}", stderr(&out));

    // Sealed delivery, when the Seal reference is importable.
    let has_seal = Command::new("python3")
        .args(["-c", "import crovia_seal"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !has_seal {
        eprintln!("skipping sealed cross-check: crovia_seal not importable");
        return;
    }
    let out = tacet(
        &bin,
        dir,
        &["keygen", "--id", "urn:test:issuer", "--out", "i.key.json"],
    );
    assert!(out.status.success());
    let out = tacet(
        &bin,
        dir,
        &[
            "prove",
            "--state",
            "state.json",
            "--sheet",
            "sheet.json",
            "--asset",
            "stripe=safe.txt",
            "--seal-key",
            "i.key.json",
            "--out",
            "sealed.json",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let out = re(
        dir,
        &[
            "pnx",
            "verify",
            "sealed.json",
            "--asset",
            "stripe=safe.txt",
            "--json",
        ],
    );
    let report: Value = json_report(&out);
    assert_eq!(report["ok"], true, "{report}");
    assert_eq!(report["sealed"], true);
    assert_eq!(report["seal_ok"], true);
    assert_eq!(report["issuer_id"], "urn:test:issuer");
    assert_eq!(code(&out), 0);
    let out = re(
        dir,
        &["pnx", "verify", "sealed.json", "--asset", "stripe=safe.txt"],
    );
    assert!(stdout(&out).contains("sealed by urn:test:issuer"));

    // The Seal binds the proof: a changed inner verdict breaks both.
    let mut bundle: Value =
        serde_json::from_str(&fs::read_to_string(dir.join("sealed.json")).unwrap()).unwrap();
    bundle["proof"]["verdict"] = json!("present");
    fs::write(
        dir.join("tampered.json"),
        serde_json::to_vec(&bundle).unwrap(),
    )
    .unwrap();
    let out = re(
        dir,
        &[
            "pnx",
            "verify",
            "tampered.json",
            "--asset",
            "stripe=safe.txt",
            "--json",
        ],
    );
    assert_eq!(code(&out), 2);
    let report: Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(report["seal_ok"], false);
    assert!(
        report["errors"]
            .to_string()
            .contains("output_hash does not bind the proof")
    );
}
