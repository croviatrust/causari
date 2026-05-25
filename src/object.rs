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

    /// Shell exit code, when the action was a command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,

    /// ISO-8601 UTC creation timestamp.
    pub created_at: String,
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
    if prefix.len() < 4 {
        return Err(anyhow!("id prefix too short, need at least 4 chars"));
    }
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
