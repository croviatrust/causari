use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Causari objects are content-addressable, identified by BLAKE3(content).
///
/// We have four object kinds:
/// - `blob`: raw file bytes
/// - `tree`: directory listing (name -> entry)
/// - `snapshot`: pointer to the root tree of the working dir at a moment
/// - `event`: an agent action (with parent event + pre/post snapshots)
///
/// Trees, snapshots and events are stored as canonical JSON
/// (sorted keys, no extra whitespace) so the hash is deterministic.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObjectKind {
    Blob,
    Tree,
    Snapshot,
    Event,
}

impl ObjectKind {
    #[allow(dead_code)] // public API helper, will be used by the upcoming TUI
    pub fn as_str(&self) -> &'static str {
        match self {
            ObjectKind::Blob => "blob",
            ObjectKind::Tree => "tree",
            ObjectKind::Snapshot => "snapshot",
            ObjectKind::Event => "event",
        }
    }
}

/// Entry inside a tree object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeEntry {
    /// "blob" or "tree"
    pub kind: String,
    /// hex BLAKE3 of the referenced object
    pub id: String,
    /// Executable bit of a blob (git's 100755 vs 100644). Only the bit is
    /// stored, never the full mode: full modes depend on the umask of the
    /// machine that took the snapshot and would make identical content hash
    /// to different trees. Absent when false, so trees written by older
    /// binaries keep their ids and old readers ignore it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub exec: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl TreeEntry {
    pub fn blob(id: String, exec: bool) -> Self {
        Self {
            kind: "blob".to_string(),
            id,
            exec,
        }
    }

    pub fn tree(id: String) -> Self {
        Self {
            kind: "tree".to_string(),
            id,
            exec: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tree {
    /// Sorted by key for deterministic serialization.
    pub entries: BTreeMap<String, TreeEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// id of the root tree
    pub tree: String,
    /// timestamp ISO-8601 UTC
    pub created_at: String,
}

/// Rich, replayable record of a single agent action.
///
/// Causari's bet is that the *intent* behind an action matters as much as the
/// bytes it produced. Every event therefore carries the prompt, the reasoning,
/// the model, the files the agent inspected, and the files it wrote. This is
/// what powers `re why <file>:<line>` and future replay/fork features.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub schema: String,

    /// Parent event id. None only for the very first event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,

    /// Agent identifier (e.g. "claude-3.5-sonnet", "gpt-4o", "cline").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,

    /// Underlying model id when distinct from the agent (e.g. "anthropic/claude-3-5-sonnet-20241022").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Tool used, e.g. "edit_file", "write_to_file", "run_command".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,

    /// Short, human-readable summary of the action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    /// The user-facing prompt or task that triggered this action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,

    /// The agent's chain-of-thought / reasoning, if exposed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,

    /// Files the agent read or considered as context.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reads: Vec<String>,

    /// Files the agent (claims to have) written.
    /// Causari verifies this against the actual snapshot diff.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub writes: Vec<String>,

    /// Token usage / cost, if reported by the agent runtime.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens_in: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens_out: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,

    /// State of the workspace BEFORE the action.
    pub pre_snapshot: String,

    /// State of the workspace AFTER the action.
    pub post_snapshot: String,

    /// Shell exit code supplied by the recorder, when the action was a command.
    /// Absent when the recorder did not send one. Never narrowed from a wider integer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,

    /// ISO-8601 UTC creation timestamp.
    pub created_at: String,

    /// How this event's attribution was obtained. Absent on events written
    /// by older binaries (which leaves their object ids unchanged).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<Evidence>,

    /// Recognised secrets replaced by `[redacted:<kind>]` in `message`,
    /// `prompt` and `reasoning` before the event was written. Absent when
    /// zero, so the object ids of untouched events are unchanged.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub redactions: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// `exit_code` from a recorder payload. Missing and JSON null stay absent.
/// Any other value must be an integer in the `i32` range. This does not run
/// a command and does not check that a process exited with the number.
pub fn declared_exit_code(args: &serde_json::Value) -> Result<Option<i32>> {
    let Some(v) = args.get("exit_code") else {
        return Ok(None);
    };
    if v.is_null() {
        return Ok(None);
    }
    let Some(n) = v.as_i64() else {
        return Err(anyhow!(
            "exit_code must be an integer from {} to {} inclusive; the call records nothing",
            i32::MIN,
            i32::MAX
        ));
    };
    i32::try_from(n).map(Some).map_err(|_| {
        anyhow!(
            "exit_code {n} is outside the signed 32-bit range {}..={}; the call records nothing",
            i32::MIN,
            i32::MAX
        )
    })
}

impl Event {
    /// Whether any text field holds a recognised secret.
    pub fn has_secrets(&self) -> bool {
        [&self.message, &self.prompt, &self.reasoning]
            .into_iter()
            .flatten()
            .any(|t| crate::redact::redact(t).1 > 0)
    }

    /// Replace recognised secrets in the text fields and count them.
    pub fn redact_secrets(&mut self) {
        let mut n = 0;
        crate::redact::redact_opt(&mut self.message, &mut n);
        crate::redact::redact_opt(&mut self.prompt, &mut n);
        crate::redact::redact_opt(&mut self.reasoning, &mut n);
        self.redactions += n;
    }
}

/// The evidence class of an event's attribution: what the reader is being
/// asked to believe, and on what basis. Every consumer that prints an
/// attribution prints this next to it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "class", rename_all = "snake_case")]
pub enum Evidence {
    /// The agent runtime stated what it did (a lifecycle hook, an MCP call,
    /// `re record`). Prompt and path are exact; the snapshot may still carry
    /// unrelated changes made since the previous event.
    Declared { source: String },
    /// A proxy-captured completion was joined to the file change by content
    /// overlap: `matched` of `considered` inserted lines were found inside
    /// the completion. A score, not a fact.
    Correlated {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exchange_id: Option<String>,
        matched: usize,
        considered: usize,
    },
    /// An observer recorded the change without any link to a cause
    /// (`re watch` with no matching completion).
    Observed { source: String },
}

impl Evidence {
    pub fn declared(source: &str) -> Self {
        Evidence::Declared {
            source: source.to_string(),
        }
    }
    pub fn observed(source: &str) -> Self {
        Evidence::Observed {
            source: source.to_string(),
        }
    }
    /// One-word label for terminal output.
    pub fn label(&self) -> &'static str {
        match self {
            Evidence::Declared { .. } => "declared",
            Evidence::Correlated { .. } => "correlated",
            Evidence::Observed { .. } => "observed",
        }
    }
    /// One line for humans: class, source or score.
    pub fn describe(&self) -> String {
        match self {
            Evidence::Declared { source } => format!("declared by {source}"),
            Evidence::Correlated {
                matched,
                considered,
                ..
            } => {
                let pct = if *considered > 0 {
                    (*matched as f64 / *considered as f64 * 100.0).round() as u32
                } else {
                    0
                };
                format!("correlated ({matched}/{considered} lines, {pct}%)")
            }
            Evidence::Observed { source } => format!("observed by {source}, cause unknown"),
        }
    }
}

/// Canonical JSON serialization (sorted keys, compact).
/// Used for deterministic hashing of structured objects.
pub fn canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    // serde_json with BTreeMap fields gives sorted keys naturally;
    // for safety we also re-serialize via serde_json::Value to enforce ordering.
    let v = serde_json::to_value(value)?;
    let sorted = sort_value(v);
    let s = serde_json::to_string(&sorted)?;
    Ok(s.into_bytes())
}

fn sort_value(v: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match v {
        Value::Object(map) => {
            let mut sorted = serde_json::Map::new();
            let mut keys: Vec<String> = map.keys().cloned().collect();
            keys.sort();
            for k in keys {
                if let Some(val) = map.get(&k) {
                    sorted.insert(k, sort_value(val.clone()));
                }
            }
            Value::Object(sorted)
        }
        Value::Array(arr) => Value::Array(arr.into_iter().map(sort_value).collect()),
        other => other,
    }
}

/// Compute BLAKE3 of a byte slice and return hex string.
pub fn hash_bytes(data: &[u8]) -> String {
    blake3::hash(data).to_hex().to_string()
}

/// Resolve a possibly-short id into a full id by scanning objects dir.
pub fn resolve_id(objects_dir: &std::path::Path, prefix: &str) -> Result<String> {
    // Ids are lowercase hex. Anything else is rejected up front so the byte
    // slicing below can never split a multi-byte character (`re show €abc`
    // used to panic here).
    if !prefix.is_ascii() || !prefix.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(anyhow!(
            "invalid id '{}': ids are hexadecimal (0-9 a-f)",
            prefix
        ));
    }
    if prefix.len() < 4 {
        return Err(anyhow!("id prefix too short, need at least 4 chars"));
    }
    if prefix.len() > 64 {
        return Err(anyhow!("id prefix too long ({} > 64 chars)", prefix.len()));
    }
    let prefix = prefix.to_ascii_lowercase();
    let prefix = prefix.as_str();
    if prefix.len() == 64 {
        return Ok(prefix.to_string());
    }
    let bucket = &prefix[..2];
    let rest = &prefix[2..];
    let bucket_dir = objects_dir.join(bucket);
    if !bucket_dir.is_dir() {
        return Err(anyhow!("no object matches '{}'", prefix));
    }
    let mut matches = Vec::new();
    for entry in std::fs::read_dir(&bucket_dir)
        .with_context(|| format!("reading {}", bucket_dir.display()))?
    {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(rest) {
            matches.push(format!("{}{}", bucket, name));
        }
    }
    match matches.len() {
        0 => Err(anyhow!("no object matches '{}'", prefix)),
        1 => Ok(matches.remove(0)),
        _ => Err(anyhow!(
            "ambiguous id '{}', matches {} objects",
            prefix,
            matches.len()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolve_id_rejects_non_hex_and_non_ascii_without_panicking() {
        let tmp = tempfile::tempdir().unwrap();
        for bad in ["€abc", "zzzz", "ab", "éééé", "12 4", &"a".repeat(65)] {
            let err = resolve_id(tmp.path(), bad).unwrap_err();
            assert!(!err.to_string().is_empty(), "{}", bad);
        }
        // A full 64-hex id passes through untouched (case-folded).
        let full = "AB".repeat(32);
        assert_eq!(resolve_id(tmp.path(), &full).unwrap(), full.to_lowercase());
    }

    #[test]
    fn canonical_json_sorts_keys_at_every_level() {
        let v = json!({"z": 1, "a": {"y": 2, "b": [ {"k": 1, "c": 2} ]}});
        let bytes = canonical_json(&v).unwrap();
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            r#"{"a":{"b":[{"c":2,"k":1}],"y":2},"z":1}"#
        );
    }

    #[test]
    fn canonical_json_is_deterministic_for_events() {
        let ev = Event {
            schema: "causari.event.v0.2".into(),
            parent: Some("p".into()),
            agent: Some("a".into()),
            model: None,
            tool: Some("edit".into()),
            message: Some("msg".into()),
            prompt: None,
            reasoning: None,
            reads: vec!["x.rs".into()],
            writes: vec!["y.rs".into()],
            tokens_in: Some(1),
            tokens_out: None,
            cost_usd: None,
            pre_snapshot: "s1".into(),
            post_snapshot: "s2".into(),
            exit_code: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            evidence: None,
            redactions: 0,
        };
        let a = canonical_json(&ev).unwrap();
        let b = canonical_json(&ev.clone()).unwrap();
        assert_eq!(a, b);
        assert_eq!(hash_bytes(&a), hash_bytes(&b));
    }

    #[test]
    fn hash_bytes_is_stable_blake3() {
        // Pin the algorithm: changing it would silently break every existing
        // repository, so this test is the canary.
        assert_eq!(
            hash_bytes(b"causari"),
            blake3::hash(b"causari").to_hex().to_string()
        );
        assert_ne!(hash_bytes(b"a"), hash_bytes(b"b"));
    }

    #[test]
    fn resolve_id_full_and_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let objects = tmp.path();
        let full = "ab".to_string() + &"c".repeat(62);
        std::fs::create_dir_all(objects.join("ab")).unwrap();
        std::fs::write(objects.join("ab").join(&full[2..]), b"x").unwrap();

        // Full 64-char id passes through untouched.
        assert_eq!(resolve_id(objects, &full).unwrap(), full);
        // A short prefix resolves to the full id.
        assert_eq!(resolve_id(objects, &full[..8]).unwrap(), full);
    }

    #[test]
    fn resolve_id_rejects_short_missing_and_ambiguous() {
        let tmp = tempfile::tempdir().unwrap();
        let objects = tmp.path();
        std::fs::create_dir_all(objects.join("ab")).unwrap();
        std::fs::write(objects.join("ab").join("cd1111"), b"x").unwrap();
        std::fs::write(objects.join("ab").join("cd2222"), b"x").unwrap();

        assert!(resolve_id(objects, "ab").is_err()); // too short
        assert!(resolve_id(objects, "ffff").is_err()); // no match
        assert!(resolve_id(objects, "abcd").is_err()); // ambiguous
        assert!(resolve_id(objects, "abcd1").is_ok());
    }
}
