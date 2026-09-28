//! The reach record of a PNX run sheet (`crovia.pnx.reach.v1`, PNX.md §4a).
//!
//! The fingerprints of a run sheet say what did not leave. The reach record
//! says *where* the run connected: every destination the witness saw the run
//! try to reach, with the outcome, and the policy the witness applied, bound
//! by hash before the run. It is an optional member `reach` of the run sheet,
//! covered by the sheet signature like every other member.
//!
//! This module builds the record from connection attempts ([`ReachLog`]),
//! evaluates a policy document ([`Policy`]) and verifies a record
//! ([`verify_reach`]), with or without the policy document. It is a port of
//! `tacet/reach.py` from `crovia-tacet`: same checks, same order, same
//! wording, asserted by the `pnx_005_reach.json` vector.

use anyhow::{Result, anyhow, bail};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

use crate::seal::csc1_serialize;

pub const REACH_VERSION: &str = "crovia.pnx.reach.v1";
pub const POLICY_VERSION: &str = "crovia.pnx.policy.v1";
pub const DOMAIN_REACH: &[u8] = b"CROVIA-PNX-REACH-v1\n";

pub const CAPTURES: [&str; 3] = ["proxy-connect", "proxy-http", "socket"];
pub const DISCLOSURES: [&str; 2] = ["clear", "salted"];
pub const OUTCOMES: [&str; 3] = ["allowed", "blocked", "failed"];
pub const POLICY_KINDS: [&str; 2] = ["allowlist", "none"];
pub const POLICY_MODES: [&str; 2] = ["enforce", "observe"];

pub const VERDICT_WITHIN: &str = "within-policy";
pub const VERDICT_OUTSIDE: &str = "outside-policy";
pub const VERDICT_UNCHECKED: &str = "unchecked";
pub const VERDICT_UNPOLICED: &str = "unpoliced";

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

fn split_rule(rule: &str) -> (String, Option<u16>) {
    if let Some((host, port)) = rule.rsplit_once(':') {
        if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) {
            // A port that does not fit u16 can never match; keep the rule intact.
            return (host.to_lowercase(), Some(port.parse().unwrap_or(0)));
        }
    }
    (rule.to_lowercase(), None)
}

/// One rule of a policy document against one destination.
///
/// `host` or `host:port`; a host beginning with `*.` matches any name that
/// ends with the rest of the rule (one or more labels), never the apex
/// itself. Case-insensitive; a rule without a port matches any port.
pub fn rule_matches(rule: &str, host: &str, port: u16) -> bool {
    let (rhost, rport) = split_rule(rule);
    let host = host.to_lowercase();
    if rport.is_some_and(|p| p != port) {
        return false;
    }
    if let Some(rest) = rhost.strip_prefix('*') {
        if rest.starts_with('.') {
            return host.ends_with(rest) && host.len() > rest.len();
        }
    }
    host == rhost
}

/// A `crovia.pnx.policy.v1` document: the allowlist the witness applies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    pub allow: Vec<String>,
}

impl Policy {
    pub fn from_json(doc: &Value) -> Result<Self> {
        if doc.get("version").and_then(Value::as_str) != Some(POLICY_VERSION) {
            bail!(
                "unknown policy version {}",
                doc.get("version").unwrap_or(&Value::Null)
            );
        }
        let allow = doc
            .get("allow")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("policy.allow must be a list of non-empty strings"))?;
        let mut rules = Vec::with_capacity(allow.len());
        for r in allow {
            match r.as_str() {
                Some(s) if !s.is_empty() => rules.push(s.to_string()),
                _ => bail!("policy.allow must be a list of non-empty strings"),
            }
        }
        Ok(Self { allow: rules })
    }

    pub fn load(path: &std::path::Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| anyhow!("reading policy {}: {e}", path.display()))?;
        let doc: Value = serde_json::from_str(&raw)
            .map_err(|e| anyhow!("parsing policy {}: {e}", path.display()))?;
        Self::from_json(&doc)
    }

    pub fn to_json(&self) -> Value {
        json!({"version": POLICY_VERSION, "allow": self.allow})
    }

    /// `sha256:` + SHA-256 of the CSC-1 encoding of the document.
    pub fn hash(&self) -> String {
        let bytes = csc1_serialize(&self.to_json()).expect("policy document is canonical JSON");
        format!("sha256:{}", hex::encode(Sha256::digest(&bytes)))
    }

    pub fn allows(&self, host: &str, port: u16) -> bool {
        self.allow.iter().any(|r| rule_matches(r, host, port))
    }

    pub fn has_wildcards(&self) -> bool {
        self.allow.iter().any(|r| split_rule(r).0.starts_with("*."))
    }
}

/// Salted disclosure of a host name: hex SHA-256 of domain ‖ salt ‖ lower-cased host.
pub fn host_hash(salt: &[u8], host: &str) -> String {
    let mut h = Sha256::new();
    h.update(DOMAIN_REACH);
    h.update(salt);
    h.update(host.to_lowercase().as_bytes());
    hex::encode(h.finalize())
}

// ---------------------------------------------------------------------------
// Log → record
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Dest {
    host: String,
    port: u16,
    outcome: &'static str,
    connections: u64,
    bytes_out: u64,
    bytes_in: u64,
    ips: BTreeSet<String>,
    first_at: String,
    last_at: String,
}

fn outcome_str(s: &str) -> Option<&'static str> {
    OUTCOMES.iter().copied().find(|o| *o == s)
}

fn merge_outcome(a: &'static str, b: &'static str) -> &'static str {
    if a == "blocked" || b == "blocked" {
        "blocked"
    } else if a == "allowed" || b == "allowed" {
        "allowed"
    } else {
        "failed"
    }
}

/// Connection attempts of one run, as the witness saw them, aggregated per
/// destination.
///
/// Feed it one [`attempt`](Self::attempt) per connection: host, port, RFC
/// 3339 time, and either the outcome the witness decided (`allowed` /
/// `blocked` / `failed`) or, when a policy is set and no outcome is given,
/// the outcome the policy implies for the mode (`enforce`: refused
/// destinations are `blocked`; `observe`: everything is `allowed`, and what
/// the policy would have said is left to the verifier). Attempts to the same
/// `(host, port)` merge; the outcome of a destination is `blocked` if any
/// attempt was blocked, else `failed` if none was relayed, else `allowed`.
#[derive(Clone, Debug)]
pub struct ReachLog {
    pub capture: String,
    pub policy: Option<Policy>,
    pub mode: String,
    dests: BTreeMap<(String, u16), Dest>,
}

impl ReachLog {
    pub fn new(capture: &str, policy: Option<Policy>, mode: &str) -> Result<Self> {
        if !CAPTURES.contains(&capture) {
            bail!("unknown capture {capture:?}");
        }
        if !POLICY_MODES.contains(&mode) {
            bail!("unknown policy mode {mode:?}");
        }
        let mode = if policy.is_none() { "observe" } else { mode };
        Ok(Self {
            capture: capture.to_string(),
            policy,
            mode: mode.to_string(),
            dests: BTreeMap::new(),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.dests.is_empty()
    }

    pub fn destinations(&self) -> usize {
        self.dests.len()
    }

    /// What the witness does with a connection to host:port under its
    /// policy and mode.
    pub fn decide(&self, host: &str, port: u16) -> &'static str {
        match &self.policy {
            Some(p) if self.mode == "enforce" && !p.allows(host, port) => "blocked",
            _ => "allowed",
        }
    }

    /// Record one connection attempt. Returns the outcome recorded.
    #[allow(clippy::too_many_arguments)]
    pub fn attempt(
        &mut self,
        host: &str,
        port: u16,
        at: &str,
        outcome: Option<&str>,
        bytes_out: u64,
        bytes_in: u64,
        ip: Option<&str>,
    ) -> Result<&'static str> {
        let host = host.to_lowercase();
        let outcome = match outcome {
            None => self.decide(&host, port),
            Some(o) => outcome_str(o).ok_or_else(|| anyhow!("unknown outcome {o:?}"))?,
        };
        if self.policy.is_none() && outcome == "blocked" {
            bail!("blocked outcome without a policy");
        }
        let d = self
            .dests
            .entry((host.clone(), port))
            .or_insert_with(|| Dest {
                host,
                port,
                outcome,
                connections: 0,
                bytes_out: 0,
                bytes_in: 0,
                ips: BTreeSet::new(),
                first_at: at.to_string(),
                last_at: at.to_string(),
            });
        d.outcome = if d.connections == 0 {
            outcome
        } else {
            merge_outcome(d.outcome, outcome)
        };
        d.connections += 1;
        if outcome != "blocked" {
            d.bytes_out += bytes_out;
            d.bytes_in += bytes_in;
        }
        if let Some(ip) = ip.filter(|s| !s.is_empty()) {
            d.ips.insert(ip.to_string());
        }
        if at < d.first_at.as_str() {
            d.first_at = at.to_string();
        }
        if at > d.last_at.as_str() {
            d.last_at = at.to_string();
        }
        Ok(outcome)
    }

    /// The `reach` member of the run sheet.
    pub fn record(&self, salt: &[u8], disclosure: &str) -> Result<Value> {
        if !DISCLOSURES.contains(&disclosure) {
            bail!("unknown disclosure {disclosure:?}");
        }
        let mut entries: Vec<Value> = Vec::with_capacity(self.dests.len());
        for d in self.dests.values() {
            let mut e = json!({
                "port": d.port,
                "outcome": d.outcome,
                "connections": d.connections,
                "bytes_out": d.bytes_out,
                "bytes_in": d.bytes_in,
                "ips": d.ips.iter().collect::<Vec<_>>(),
                "first_at": d.first_at,
                "last_at": d.last_at,
            });
            if disclosure == "salted" {
                e["host_hash"] = json!(host_hash(salt, &d.host));
            } else {
                e["host"] = json!(d.host);
            }
            entries.push(e);
        }
        if disclosure == "salted" {
            entries.sort_by(|a, b| {
                (a["host_hash"].as_str(), a["port"].as_u64())
                    .cmp(&(b["host_hash"].as_str(), b["port"].as_u64()))
            });
        }
        let policy = match &self.policy {
            Some(p) => {
                json!({"kind": "allowlist", "mode": self.mode, "hash": p.hash(), "rules": p.allow.len()})
            }
            None => json!({"kind": "none", "mode": "observe", "hash": Value::Null, "rules": 0}),
        };
        let summary = summarize(&entries);
        Ok(json!({
            "version": REACH_VERSION,
            "capture": self.capture,
            "disclosure": disclosure,
            "policy": policy,
            "destinations": entries,
            "summary": summary,
        }))
    }
}

pub fn summarize(entries: &[Value]) -> Value {
    let count = |o: &str| {
        entries
            .iter()
            .filter(|e| e.get("outcome").and_then(Value::as_str) == Some(o))
            .count()
    };
    json!({
        "destinations": entries.len(),
        "connections": entries.iter().map(|e| e.get("connections").and_then(Value::as_u64).unwrap_or(0)).sum::<u64>(),
        "allowed": count("allowed"),
        "blocked": count("blocked"),
        "failed": count("failed"),
    })
}

// ---------------------------------------------------------------------------
// Verification (PNX.md §6 step 1b)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct ReachVerifyResult {
    pub ok: bool,
    pub verdict: String,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    /// `host:port` of allowed/failed destinations no rule matches.
    pub outside: Vec<String>,
    /// Salted disclosure: name held by the verifier → reached.
    pub reached: BTreeMap<String, bool>,
}

fn is_rfc3339(v: Option<&Value>) -> bool {
    v.and_then(Value::as_str)
        .is_some_and(|s| s.len() >= 20 && s.ends_with('Z') && s.as_bytes()[10] == b'T')
}

fn non_negative_int(v: Option<&Value>) -> Option<u64> {
    v.and_then(Value::as_u64)
}

/// Verify a reach record.
///
/// `salt` is the run salt of the sheet (for salted disclosure). With the
/// policy document the hash is recomputed and, under clear disclosure, every
/// destination is matched against it. `names` are host names the verifier
/// holds and wants checked under salted disclosure.
pub fn verify_reach(
    reach: &Value,
    salt: &[u8],
    policy: Option<&Policy>,
    names: &[String],
) -> ReachVerifyResult {
    let mut res = ReachVerifyResult {
        verdict: "?".to_string(),
        ..Default::default()
    };
    let empty = Map::new();
    let obj = reach.as_object().unwrap_or(&empty);
    let s = |k: &str| obj.get(k).and_then(Value::as_str);
    if s("version") != Some(REACH_VERSION) {
        res.errors.push(format!(
            "unknown reach version {}",
            obj.get("version").unwrap_or(&Value::Null)
        ));
        return res;
    }
    let capture = s("capture").unwrap_or("");
    let disclosure = s("disclosure").unwrap_or("");
    if !CAPTURES.contains(&capture) {
        res.errors.push(format!("unknown capture {capture:?}"));
    }
    if !DISCLOSURES.contains(&disclosure) {
        res.errors
            .push(format!("unknown disclosure {disclosure:?}"));
        return res;
    }
    let pol = obj
        .get("policy")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let kind = pol.get("kind").and_then(Value::as_str).unwrap_or("");
    let mode = pol.get("mode").and_then(Value::as_str).unwrap_or("");
    let phash = pol.get("hash").cloned().unwrap_or(Value::Null);
    let rules = pol.get("rules").and_then(Value::as_u64);
    if !POLICY_KINDS.contains(&kind) {
        res.errors.push(format!("unknown policy kind {kind:?}"));
    }
    if !POLICY_MODES.contains(&mode) {
        res.errors.push(format!("unknown policy mode {mode:?}"));
    }
    if kind == "none" && (!phash.is_null() || rules != Some(0) || mode != "observe") {
        res.errors
            .push("policy kind none must have hash null, rules 0, mode observe".to_string());
    }
    let hash_ok = phash
        .as_str()
        .is_some_and(|h| h.starts_with("sha256:") && h.len() == 71);
    if kind == "allowlist" && !(hash_ok && rules.is_some()) {
        res.errors
            .push("policy kind allowlist must carry a sha256: hash and a rule count".to_string());
    }

    let Some(entries) = obj.get("destinations").and_then(Value::as_array) else {
        res.errors.push("destinations must be a list".to_string());
        return res;
    };
    let (name_key, other) = if disclosure == "salted" {
        ("host_hash", "host")
    } else {
        ("host", "host_hash")
    };
    let mut keys: Vec<(String, u64)> = Vec::new();
    let mut objects: Vec<Value> = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        let Some(e) = e.as_object() else {
            res.errors.push(format!("destination {i}: not an object"));
            continue;
        };
        objects.push(Value::Object(e.clone()));
        if e.contains_key(other) {
            res.errors.push(format!(
                "destination {i}: {other} not allowed under {disclosure} disclosure"
            ));
        }
        let name = match e.get(name_key).and_then(Value::as_str) {
            Some(n) if !n.is_empty() => n,
            _ => {
                res.errors
                    .push(format!("destination {i}: missing {name_key}"));
                continue;
            }
        };
        if disclosure == "salted"
            && (name.len() != 64
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        {
            res.errors.push(format!(
                "destination {i}: host_hash is not 64 lowercase hex characters"
            ));
        }
        if disclosure == "clear" && name != name.to_lowercase() {
            res.errors
                .push(format!("destination {i}: host must be lower-cased"));
        }
        let port = match e.get("port").and_then(Value::as_u64) {
            Some(p) if (1..65536).contains(&p) => p,
            _ => {
                res.errors
                    .push(format!("destination {i}: port out of range"));
                continue;
            }
        };
        let outcome = e.get("outcome").and_then(Value::as_str).unwrap_or("");
        if !OUTCOMES.contains(&outcome) {
            res.errors.push(format!(
                "destination {i}: unknown outcome {}",
                e.get("outcome").unwrap_or(&Value::Null)
            ));
        }
        if kind == "none" && outcome == "blocked" {
            res.errors
                .push(format!("destination {i}: blocked without a policy"));
        }
        if mode == "observe" && outcome == "blocked" {
            res.errors
                .push(format!("destination {i}: blocked under observe mode"));
        }
        for k in ["connections", "bytes_out", "bytes_in"] {
            if non_negative_int(e.get(k)).is_none() {
                res.errors.push(format!(
                    "destination {i}: {k} must be a non-negative integer"
                ));
            }
        }
        if non_negative_int(e.get("connections")).unwrap_or(0) < 1 {
            res.errors
                .push(format!("destination {i}: connections must be at least 1"));
        }
        let relayed = non_negative_int(e.get("bytes_out")).unwrap_or(0)
            + non_negative_int(e.get("bytes_in")).unwrap_or(0);
        if outcome == "blocked" && relayed > 0 {
            res.errors.push(format!(
                "destination {i}: bytes relayed on a blocked destination"
            ));
        }
        let ips_ok = e.get("ips").and_then(Value::as_array).is_some_and(|ips| {
            let strs: Vec<&str> = ips.iter().filter_map(Value::as_str).collect();
            strs.len() == ips.len() && strs.windows(2).all(|w| w[0] < w[1])
        });
        if !ips_ok {
            res.errors.push(format!(
                "destination {i}: ips must be a sorted list without duplicates"
            ));
        }
        let first = e.get("first_at");
        let last = e.get("last_at");
        if !is_rfc3339(first)
            || !is_rfc3339(last)
            || first.and_then(Value::as_str) > last.and_then(Value::as_str)
        {
            res.errors.push(format!(
                "destination {i}: first_at/last_at must be RFC 3339 UTC and ordered"
            ));
        }
        keys.push((name.to_string(), port));
    }
    let mut sorted = keys.clone();
    sorted.sort();
    if keys != sorted {
        res.errors
            .push("destinations are not sorted by host then port".to_string());
    }
    if keys.iter().collect::<BTreeSet<_>>().len() != keys.len() {
        res.errors.push("duplicate destination".to_string());
    }
    if obj.get("summary") != Some(&summarize(&objects)) {
        res.errors
            .push("summary does not match the destinations".to_string());
    }
    if !res.errors.is_empty() {
        return res;
    }

    // Policy conformance.
    if kind == "none" {
        res.verdict = VERDICT_UNPOLICED.to_string();
        if policy.is_some() {
            res.warnings.push(
                "policy document supplied but the record was made without a policy".to_string(),
            );
        }
    } else if let Some(policy) = policy {
        let h = policy.hash();
        if Some(h.as_str()) != phash.as_str() || Some(policy.allow.len() as u64) != rules {
            res.errors.push(format!(
                "policy document does not match the record: hash {h} vs {}, {} rules vs {}",
                phash.as_str().unwrap_or("null"),
                policy.allow.len(),
                rules
                    .map(|r| r.to_string())
                    .unwrap_or_else(|| "null".into())
            ));
            return res;
        }
        if disclosure == "clear" {
            for e in entries {
                let host = e["host"].as_str().unwrap_or("");
                let port = e["port"].as_u64().unwrap_or(0) as u16;
                let outcome = e["outcome"].as_str().unwrap_or("");
                let allowed = policy.allows(host, port);
                if (outcome == "allowed" || outcome == "failed") && !allowed {
                    res.outside.push(format!("{host}:{port}"));
                }
                if outcome == "blocked" && allowed {
                    res.errors.push(format!(
                        "{host}:{port}: blocked although the policy allows it (inconsistent witness)"
                    ));
                }
            }
            if !res.errors.is_empty() {
                return res;
            }
            res.verdict = if res.outside.is_empty() {
                VERDICT_WITHIN
            } else {
                VERDICT_OUTSIDE
            }
            .to_string();
        } else {
            res.verdict = if mode == "enforce" {
                VERDICT_WITHIN
            } else {
                VERDICT_UNCHECKED
            }
            .to_string();
            res.warnings.push(format!(
                "salted disclosure: the policy hash matches; destinations cannot be matched against the rules{}",
                if policy.has_wildcards() {
                    " (the policy has wildcard rules)"
                } else {
                    ""
                }
            ));
        }
    } else if mode == "enforce" {
        res.verdict = VERDICT_WITHIN.to_string();
        res.warnings.push(
            "policy document not supplied: the outcomes rest on the witness; only the policy hash is bound"
                .to_string(),
        );
    } else {
        res.verdict = VERDICT_UNCHECKED.to_string();
        res.warnings.push(
            "policy document not supplied and the witness observed only: conformance cannot be checked"
                .to_string(),
        );
    }
    if disclosure == "salted" {
        let hashes: BTreeSet<&str> = entries
            .iter()
            .filter_map(|e| e["host_hash"].as_str())
            .collect();
        for n in names {
            res.reached
                .insert(n.clone(), hashes.contains(host_hash(salt, n).as_str()));
        }
    }
    res.ok = res.errors.is_empty();
    res
}

// ---------------------------------------------------------------------------
// Tests: the pnx_005_reach vector, as the Python and JavaScript runners read it
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn vector() -> Value {
        serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/vectors/pnx/conformance/pnx_005_reach.json"
        )))
        .unwrap()
    }

    fn salt_of(v: &Value) -> Vec<u8> {
        hex::decode(v["valid"]["enforce"]["sheet"]["salt_hex"].as_str().unwrap()).unwrap()
    }

    fn names_of(vec: &Value) -> Vec<String> {
        vec.get("names")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn rules_match_hosts_and_ports_as_the_spec_says() {
        assert!(rule_matches("api.github.com:443", "API.GITHUB.COM", 443));
        assert!(!rule_matches("api.github.com:443", "api.github.com", 80));
        assert!(rule_matches("github.com", "github.com", 22));
        assert!(rule_matches(
            "*.githubusercontent.com:443",
            "raw.githubusercontent.com",
            443
        ));
        assert!(rule_matches(
            "*.githubusercontent.com:443",
            "a.b.githubusercontent.com",
            443
        ));
        assert!(!rule_matches(
            "*.githubusercontent.com:443",
            "githubusercontent.com",
            443
        ));
        assert!(!rule_matches("*.github.com", "evilgithub.com", 443));
        assert!(!rule_matches(
            "api.github.com",
            "api.github.com.evil.example",
            443
        ));
    }

    #[test]
    fn policy_hash_and_host_hashes_match_the_vector() {
        let v = vector();
        let policy = Policy::from_json(&v["policy"]["document"]).unwrap();
        assert_eq!(policy.hash(), v["policy"]["hash"]);
        assert_eq!(
            policy.allow.len() as u64,
            v["policy"]["rules"].as_u64().unwrap()
        );
        let salt = salt_of(&v);
        for (name, h) in v["host_hashes"].as_object().unwrap() {
            assert_eq!(host_hash(&salt, name), h.as_str().unwrap(), "{name}");
        }
        assert!(Policy::from_json(&json!({"version": "x", "allow": []})).is_err());
        assert!(Policy::from_json(&json!({"version": POLICY_VERSION, "allow": [""]})).is_err());
    }

    #[test]
    fn the_witness_rebuilds_the_vector_records_from_the_log() {
        let v = vector();
        let policy = Policy::from_json(&v["policy"]["document"]).unwrap();
        let salt = salt_of(&v);
        let mut log = ReachLog::new("proxy-connect", Some(policy), "enforce").unwrap();
        for a in v["log"].as_array().unwrap() {
            log.attempt(
                a["host"].as_str().unwrap(),
                a["port"].as_u64().unwrap() as u16,
                a["at"].as_str().unwrap(),
                None,
                a["bytes_out"].as_u64().unwrap(),
                a["bytes_in"].as_u64().unwrap(),
                a["ip"].as_str(),
            )
            .unwrap();
        }
        assert_eq!(
            log.record(&salt, "clear").unwrap(),
            v["valid"]["enforce"]["sheet"]["reach"]
        );
        assert_eq!(
            log.record(&salt, "salted").unwrap(),
            v["valid"]["salted"]["sheet"]["reach"]
        );
    }

    #[test]
    fn valid_records_verify_with_and_without_the_policy() {
        let v = vector();
        let policy = Policy::from_json(&v["policy"]["document"]).unwrap();
        let salt = salt_of(&v);
        for (name, vec) in v["valid"].as_object().unwrap() {
            let reach = &vec["sheet"]["reach"];
            let names = names_of(vec);
            for (label, pol) in [
                ("expect_with_policy", Some(&policy)),
                ("expect_without_policy", None),
            ] {
                let exp = &vec[label];
                let r = verify_reach(reach, &salt, pol, &names);
                assert!(r.ok, "{name} {label}: {:?}", r.errors);
                assert_eq!(r.verdict, exp["verdict"], "{name} {label}");
                assert_eq!(json!(r.outside), exp["outside"], "{name} {label}");
                assert_eq!(json!(r.reached), exp["reached"], "{name} {label}");
                if let Some(w) = exp["warning_contains"].as_str() {
                    assert!(
                        r.warnings.iter().any(|x| x.contains(w)),
                        "{name} {label}: warnings {:?} must mention {w:?}",
                        r.warnings
                    );
                }
            }
        }
    }

    #[test]
    fn every_fault_of_the_vector_is_named() {
        let v = vector();
        let policy = Policy::from_json(&v["policy"]["document"]).unwrap();
        let salt = salt_of(&v);
        let mut seen = 0;
        for (name, vec) in v["invalid"].as_object().unwrap() {
            let sheet = &vec["sheet"];
            let mut errs = crate::pnx::verify_sheet(sheet);
            if errs.is_empty() && vec["with_policy"].as_bool() == Some(true) {
                errs = verify_reach(&sheet["reach"], &salt, Some(&policy), &[]).errors;
            }
            let needle = vec["expect_error_contains"].as_str().unwrap();
            assert!(
                errs.iter().any(|e| e.contains(needle)),
                "{name}: {errs:?} must contain {needle:?}"
            );
            seen += 1;
        }
        assert!(
            seen >= 10,
            "the vector names at least 10 faults, saw {seen}"
        );
    }

    #[test]
    fn observe_mode_records_everything_and_lets_the_verifier_judge() {
        let policy = Policy {
            allow: vec!["api.openai.com:443".into()],
        };
        let mut log = ReachLog::new("proxy-http", Some(policy.clone()), "observe").unwrap();
        log.attempt(
            "api.openai.com",
            443,
            "2026-09-28T10:00:00Z",
            None,
            10,
            20,
            None,
        )
        .unwrap();
        log.attempt(
            "pastebin.com",
            443,
            "2026-09-28T10:00:01Z",
            None,
            5,
            0,
            None,
        )
        .unwrap();
        let rec = log.record(&[0u8; 16], "clear").unwrap();
        assert_eq!(rec["summary"]["blocked"], 0);
        let r = verify_reach(&rec, &[0u8; 16], Some(&policy), &[]);
        assert!(r.ok);
        assert_eq!(r.verdict, VERDICT_OUTSIDE);
        assert_eq!(r.outside, ["pastebin.com:443"]);
        let r = verify_reach(&rec, &[0u8; 16], None, &[]);
        assert_eq!(r.verdict, VERDICT_UNCHECKED);

        // Enforce mode blocks it instead and the record stays within policy.
        let mut log = ReachLog::new("proxy-http", Some(policy.clone()), "enforce").unwrap();
        assert_eq!(log.decide("pastebin.com", 443), "blocked");
        log.attempt(
            "pastebin.com",
            443,
            "2026-09-28T10:00:01Z",
            None,
            5,
            0,
            None,
        )
        .unwrap();
        let rec = log.record(&[0u8; 16], "clear").unwrap();
        assert_eq!(rec["destinations"][0]["bytes_out"], 0);
        assert_eq!(
            verify_reach(&rec, &[0u8; 16], Some(&policy), &[]).verdict,
            VERDICT_WITHIN
        );

        // No policy: unpoliced, and blocked is not a legal outcome.
        let mut log = ReachLog::new("proxy-http", None, "enforce").unwrap();
        assert_eq!(log.mode, "observe");
        assert!(
            log.attempt(
                "x.example",
                1,
                "2026-09-28T10:00:00Z",
                Some("blocked"),
                0,
                0,
                None
            )
            .is_err()
        );
        log.attempt(
            "x.example",
            1,
            "2026-09-28T10:00:00Z",
            Some("failed"),
            0,
            0,
            None,
        )
        .unwrap();
        let rec = log.record(&[0u8; 16], "clear").unwrap();
        assert_eq!(
            verify_reach(&rec, &[0u8; 16], None, &[]).verdict,
            VERDICT_UNPOLICED
        );
    }
}
