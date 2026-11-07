//! `re mcp` end to end, through the binary and the stdio transport an agent
//! runtime uses: newline-delimited JSON-RPC on stdin and stdout. The in-process
//! tests check the wording of the tool list; this one checks that a client
//! which only has the process gets the handshake, the list, and the five
//! tools, and that the answers keep the distinctions the descriptions promise.
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

use serde_json::{Value, json};

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(["-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Tarik")
        .env("GIT_AUTHOR_EMAIL", "tarik@example.com")
        .env("GIT_COMMITTER_NAME", "Tarik")
        .env("GIT_COMMITTER_EMAIL", "tarik@example.com")
        .status()
        .expect("git must be installed");
    assert!(status.success(), "git {args:?} failed");
}

fn re(dir: &Path, args: &[&str]) {
    let out = Command::new(env!("CARGO_BIN_EXE_re"))
        .args(args)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "re {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// One committed file, no AI trailer, `re init` done.
fn fixture() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    git(dir, &["init", "-q", "-b", "main"]);
    fs::write(dir.join("app.py"), "def greet(name):\n    return name\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", "initial scaffold"]);
    re(dir, &["init"]);
    temp
}

struct Server {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Server {
    fn start(dir: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_re"))
            .arg("mcp")
            .current_dir(dir)
            .env("NO_COLOR", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Server {
            child,
            stdin,
            stdout,
            next_id: 1,
        }
    }

    fn send(&mut self, msg: &Value) {
        let mut line = msg.to_string();
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).unwrap();
        self.stdin.flush().unwrap();
    }

    fn read(&mut self) -> Value {
        let mut line = String::new();
        let n = self.stdout.read_line(&mut line).unwrap();
        assert!(n > 0, "server closed stdout before answering");
        serde_json::from_str(line.trim())
            .unwrap_or_else(|e| panic!("server wrote a line that is not JSON ({e}): {line:?}"))
    }

    /// A request with an id; returns the whole envelope.
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let reply = self.read();
        assert_eq!(reply["jsonrpc"], "2.0");
        assert_eq!(
            reply["id"], id,
            "answer carries another request's id: {reply}"
        );
        reply
    }

    fn result(&mut self, method: &str, params: Value) -> Value {
        let reply = self.request(method, params);
        assert!(
            reply.get("error").is_none(),
            "{method} answered an error: {}",
            reply["error"]
        );
        reply["result"].clone()
    }

    fn error(&mut self, method: &str, params: Value) -> Value {
        let reply = self.request(method, params);
        assert!(
            reply.get("result").is_none(),
            "{method} answered a result where an error was expected: {}",
            reply["result"]
        );
        reply["error"].clone()
    }

    /// `tools/call` and the text of the first content block.
    fn call(&mut self, tool: &str, arguments: Value) -> String {
        let result = self.result(
            "tools/call",
            json!({ "name": tool, "arguments": arguments }),
        );
        assert_eq!(result["content"][0]["type"], "text");
        result["content"][0]["text"].as_str().unwrap().to_string()
    }

    fn stop(mut self) {
        drop(self.stdin);
        let status = self.child.wait().unwrap();
        assert!(status.success(), "re mcp exited {status} on EOF");
    }
}

#[test]
fn handshake_list_and_tools_over_stdio() {
    let temp = fixture();
    let dir = temp.path();
    let mut s = Server::start(dir);

    // initialize: the server names itself and its version.
    let init = s.result(
        "initialize",
        json!({ "protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": { "name": "test", "version": "0" } }),
    );
    assert_eq!(init["serverInfo"]["name"], "causari");
    assert_eq!(init["serverInfo"]["version"], env!("CARGO_PKG_VERSION"));
    assert!(init["capabilities"]["tools"].is_object());

    // A notification has no id and gets no answer; the next answer must
    // belong to the next request, not to the notification.
    s.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    let ping = s.result("ping", json!({}));
    assert!(ping.is_object());

    // tools/list: exactly the five tools the docs name, each with a schema.
    let list = s.result("tools/list", json!({}));
    let mut names: Vec<&str> = list["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "causari_audit",
            "causari_handoff",
            "causari_recall",
            "causari_record",
            "causari_why"
        ]
    );
    for t in list["tools"].as_array().unwrap() {
        assert_eq!(
            t["inputSchema"]["type"], "object",
            "{} has no object schema",
            t["name"]
        );
        assert!(t["description"].as_str().unwrap().len() > 40);
    }
    let audit_desc = list["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "causari_audit")
        .unwrap()["description"]
        .as_str()
        .unwrap()
        .to_string();

    // causari_why on a line nobody recorded: unknown, and not "human".
    let why = s.call("causari_why", json!({ "file": "app.py", "line": 1 }));
    assert!(why.contains("no recorded event"), "{why}");
    assert!(!why.to_lowercase().contains("human"), "{why}");

    // causari_record: a declared action; the reply names the event.
    fs::write(
        dir.join("app.py"),
        "def greet(name):\n    return name\n\ndef farewell(name):\n    return 'bye ' + name\n",
    )
    .unwrap();
    let recorded = s.call(
        "causari_record",
        json!({
            "message": "add farewell helper",
            "agent": "test-agent",
            "model": "test-model",
            "tool": "edit_file",
            "prompt": "add a farewell function",
            "writes": ["app.py"],
            "exit_code": 0
        }),
    );
    assert!(recorded.starts_with("recorded event "), "{recorded}");
    assert!(recorded.contains("add farewell helper"), "{recorded}");

    // causari_why on the new line: the declared event, labelled declared.
    let why = s.call("causari_why", json!({ "file": "app.py", "line": 4 }));
    assert!(why.contains("Introduced by event"), "{why}");
    assert!(why.contains("declared by mcp"), "{why}");
    assert!(why.contains("agent: test-agent"), "{why}");
    assert!(
        !why.contains("observed"),
        "a declared event must not read as observed: {why}"
    );

    // causari_recall finds the event by its words and awards nothing.
    let recall = s.call("causari_recall", json!({ "query": "farewell" }));
    assert!(recall.contains("event(s) match"), "{recall}");
    assert!(recall.contains("add farewell helper"), "{recall}");
    assert!(!recall.contains("proven"), "{recall}");
    let empty = s.call("causari_recall", json!({ "query": "" }));
    assert_eq!(empty, "recall: empty query");
    let none = s.call("causari_recall", json!({ "query": "zzqx-nothing" }));
    assert!(none.starts_with("no skills or past events match"), "{none}");

    // causari_audit on the fixture: real JSON, the method the description
    // names, and null where nothing was measured, not zero.
    let audit_text = s.call("causari_audit", json!({ "path": dir.to_string_lossy() }));
    let audit: Value = serde_json::from_str(&audit_text).expect("causari_audit returns JSON");
    let method = audit["method"].as_str().unwrap();
    assert!(
        audit_desc.contains(&format!("method {method}")),
        "tool description does not name the method the tool computes ({method}): {audit_desc}"
    );
    assert_eq!(audit["total_commits"], 1);
    assert_eq!(audit["verified"]["commits"], 0);
    assert!(
        audit["baseline"]["age_matched"].is_null(),
        "no AI-tagged commit, so the age-matched gap is null, not a number: {}",
        audit["baseline"]["age_matched"]
    );
    assert!(audit["repository"]["head"].as_str().unwrap().len() >= 40);

    s.stop();
}

#[test]
fn bad_input_is_an_error_and_records_nothing() {
    let temp = fixture();
    let dir = temp.path();
    let mut s = Server::start(dir);
    s.result("initialize", json!({}));

    // A fractional exit code is refused before any write.
    let err = s.error(
        "tools/call",
        json!({ "name": "causari_record", "arguments": { "message": "should not land", "exit_code": 1.5 } }),
    );
    assert!(
        err["message"].as_str().unwrap().contains("exit_code"),
        "{err}"
    );
    let recall = s.call("causari_recall", json!({ "query": "land" }));
    assert!(
        recall.starts_with("no skills or past events match"),
        "{recall}"
    );

    // Unknown tool, unknown method, missing tool name: errors, not silence.
    let err = s.error(
        "tools/call",
        json!({ "name": "causari_prove", "arguments": {} }),
    );
    assert!(
        err["message"].as_str().unwrap().contains("unknown tool"),
        "{err}"
    );
    let err = s.error("resources/list", json!({}));
    assert_eq!(err["code"], -32601);
    let err = s.error("tools/call", json!({ "arguments": {} }));
    assert!(
        err["message"]
            .as_str()
            .unwrap()
            .contains("missing tool name"),
        "{err}"
    );

    // A line that is not JSON is skipped, and the server keeps serving.
    s.stdin.write_all(b"this is not json\n").unwrap();
    s.stdin.flush().unwrap();
    let ping = s.result("ping", json!({}));
    assert!(ping.is_object());

    s.stop();
}

#[test]
fn handoff_over_stdio_recovers_a_receipt_after_restart() {
    let temp = fixture();
    let dir = temp.path();
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(head.status.success());
    let head = String::from_utf8(head.stdout).unwrap();
    let args = json!({"name": "causari_handoff", "arguments": {
        "slot": "test", "expected": null, "next": head.trim()
    }});
    let mut first = Server::start(dir);
    first.send(&json!({"jsonrpc": "2.0", "method": "tools/call", "params": args}));
    let accepted = first.result("tools/call", args.clone());
    assert_eq!(accepted["structuredContent"]["status"], "accepted");
    first.stop();

    let mut second = Server::start(dir);
    let replayed = second.result("tools/call", args);
    assert_eq!(replayed["structuredContent"]["status"], "replayed");
    assert_eq!(
        replayed["structuredContent"]["receipt"],
        accepted["structuredContent"]["receipt"]
    );
    let bad = second.result(
        "tools/call",
        json!({"name": "causari_handoff", "arguments": {
            "slot": "test", "next": head.trim()
        }}),
    );
    assert_eq!(bad["isError"], true);
    second.stop();
}
