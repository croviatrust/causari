//! Crovia Seal v1 issuer and verifier.
//!
//! Implements `draft-crovia-seal-01` / SPEC v0.5: CSC-1 canonicalization
//! (a strict subset of RFC 8785 that forbids floats in signed payloads),
//! Ed25519 signatures with the `CROVIA-SEAL-v1` domain prefix, and the
//! per-issuer append-only hash chain.
//!
//! This makes Causari the first production issuer of Crovia Seals: every
//! LLM exchange that flows through `re proxy --seal` leaves a compact,
//! offline-verifiable receipt in `.causari/seal/seals.jsonl`.
//!
//! Conformance is proven against the normative test vectors shipped in
//! `tests/vectors/` (copied verbatim from croviatrust/crovia-seal).

use anyhow::{Context, Result, anyhow, bail};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

use crate::repo::Repo;

pub const SEAL_VERSION: &str = "crovia.seal.v1";
pub const DOMAIN: &[u8] = b"CROVIA-SEAL-v1";
const MAX_SAFE_INT: i64 = 9_007_199_254_740_991; // 2^53 - 1
const BASE32_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

// ---------------------------------------------------------------------------
// CSC-1 canonicalization (Section 3 of the spec)
// ---------------------------------------------------------------------------

/// Serialize a JSON value to its CSC-1 canonical byte sequence.
///
/// Fails on floats, NaN/Infinity, -0, integers outside ±(2^53−1) and
/// duplicate keys (serde_json already collapses duplicates at parse time;
/// values built programmatically cannot contain them).
pub fn csc1_serialize(value: &Value) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    write_canonical(value, &mut out)?;
    Ok(out)
}

fn write_canonical(value: &Value, out: &mut Vec<u8>) -> Result<()> {
    match value {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(true) => out.extend_from_slice(b"true"),
        Value::Bool(false) => out.extend_from_slice(b"false"),
        Value::Number(n) => {
            let i = if let Some(i) = n.as_i64() {
                i
            } else if let Some(u) = n.as_u64() {
                i64::try_from(u).map_err(|_| anyhow!("NonCanonicalNumber: {} exceeds 2^53-1", u))?
            } else {
                bail!("NonCanonicalNumber: floats are forbidden in signed payloads ({})", n);
            };
            if !(-MAX_SAFE_INT..=MAX_SAFE_INT).contains(&i) {
                bail!("NonCanonicalNumber: {} outside ±(2^53-1)", i);
            }
            out.extend_from_slice(i.to_string().as_bytes());
        }
        Value::String(s) => write_canonical_string(s, out),
        Value::Array(items) => {
            out.push(b'[');
            for (idx, item) in items.iter().enumerate() {
                if idx > 0 {
                    out.push(b',');
                }
                write_canonical(item, out)?;
            }
            out.push(b']');
        }
        Value::Object(map) => {
            // Keys sorted by UTF-16 code-unit sequence (JCS / JavaScript sort).
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by(|a, b| {
                let au: Vec<u16> = a.encode_utf16().collect();
                let bu: Vec<u16> = b.encode_utf16().collect();
                au.cmp(&bu)
            });
            out.push(b'{');
            for (idx, key) in keys.iter().enumerate() {
                if idx > 0 {
                    out.push(b',');
                }
                write_canonical_string(key, out);
                out.push(b':');
                write_canonical(&map[key.as_str()], out)?;
            }
            out.push(b'}');
        }
    }
    Ok(())
}

/// RFC 8259 minimal escaping: `\" \\ \b \f \n \r \t`, `\u00XX` for other
/// control chars, everything else emitted literally as UTF-8.
fn write_canonical_string(s: &str, out: &mut Vec<u8>) {
    out.push(b'"');
    for c in s.chars() {
        match c {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\u{0008}' => out.extend_from_slice(b"\\b"),
            '\u{000C}' => out.extend_from_slice(b"\\f"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\r' => out.extend_from_slice(b"\\r"),
            '\t' => out.extend_from_slice(b"\\t"),
            c if (c as u32) < 0x20 => {
                out.extend_from_slice(format!("\\u{:04x}", c as u32).as_bytes());
            }
            c => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    out.push(b'"');
}

// ---------------------------------------------------------------------------
// Payload & helpers (Section 3.3)
// ---------------------------------------------------------------------------

/// `P(S) = DOMAIN || 0x0A || CSC1(S \ {signature, witnesses})`
pub fn signing_payload(seal: &Value) -> Result<Vec<u8>> {
    let obj = seal
        .as_object()
        .ok_or_else(|| anyhow!("seal must be a JSON object"))?;
    let mut stripped = obj.clone();
    stripped.remove("signature");
    stripped.remove("witnesses");
    let mut payload = Vec::with_capacity(512);
    payload.extend_from_slice(DOMAIN);
    payload.push(0x0A);
    payload.extend_from_slice(&csc1_serialize(&Value::Object(stripped))?);
    Ok(payload)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// RFC 4648 base32, no padding, uppercase.
fn base32_nopad(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut acc: u64 = 0;
    let mut bits = 0u32;
    for &b in bytes {
        acc = (acc << 8) | b as u64;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(BASE32_ALPHABET[((acc >> bits) & 0x1F) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(BASE32_ALPHABET[((acc << (5 - bits)) & 0x1F) as usize] as char);
    }
    out
}

fn random_base32_26() -> Result<String> {
    let mut raw = [0u8; 16];
    getrandom::getrandom(&mut raw).map_err(|e| anyhow!("secure randomness unavailable: {}", e))?;
    Ok(base32_nopad(&raw))
}

fn rfc3339_now_ms() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

// ---------------------------------------------------------------------------
// Issuer (Sections 4 & 5)
// ---------------------------------------------------------------------------

/// What a Seal describes: the exact wire-level exchange the proxy relayed.
pub struct SealSubject<'a> {
    /// Exact request bytes sent upstream.
    pub input: &'a [u8],
    /// Exact response bytes returned to the client.
    pub output: &'a [u8],
    /// One of "text","code","image","audio","multimodal".
    pub modality: &'a str,
}

pub struct SealGenerator<'a> {
    pub id: &'a str,
    pub version: Option<&'a str>,
    /// Generation parameters; values MUST already be strings (CSC-1).
    pub params: Vec<(String, String)>,
}

/// A stateful Seal issuer bound to a repository: owns the Ed25519 issuer
/// key, the chain state and the append-only seal log.
pub struct SealIssuer {
    key: SigningKey,
    issuer_id: String,
    sequence: u64,
    prev_seal_hash: Option<String>,
    state_path: PathBuf,
    log_path: PathBuf,
}

pub fn seal_dir(repo: &Repo) -> PathBuf {
    repo.dir.join("seal")
}

pub fn seals_log_path(repo: &Repo) -> PathBuf {
    seal_dir(repo).join("seals.jsonl")
}

fn state_path(repo: &Repo) -> PathBuf {
    seal_dir(repo).join("state.json")
}

fn issuer_key_path(repo: &Repo) -> PathBuf {
    repo.dir.join("keys").join("seal-issuer.key")
}

impl SealIssuer {
    /// Load the repo's seal issuer, creating key and chain state on first use.
    pub fn load_or_create(repo: &Repo, issuer_id: Option<String>) -> Result<Self> {
        let key_path = issuer_key_path(repo);
        let key = if key_path.exists() {
            let hex_str = std::fs::read_to_string(&key_path)
                .with_context(|| format!("reading {}", key_path.display()))?;
            let bytes = hex::decode(hex_str.trim()).context("decoding seal issuer key")?;
            let arr: [u8; 32] = bytes
                .try_into()
                .map_err(|_| anyhow!("seal issuer key must be 32 bytes"))?;
            SigningKey::from_bytes(&arr)
        } else {
            let mut secret = [0u8; 32];
            getrandom::getrandom(&mut secret)
                .map_err(|e| anyhow!("generating issuer key: {}", e))?;
            let key = SigningKey::from_bytes(&secret);
            std::fs::create_dir_all(key_path.parent().unwrap())?;
            std::fs::write(&key_path, hex::encode(secret))?;
            std::fs::write(
                key_path.with_extension("pub"),
                hex::encode(key.verifying_key().to_bytes()),
            )?;
            key
        };

        let sp = state_path(repo);
        let (sequence, prev_seal_hash) = if sp.exists() {
            let raw = std::fs::read_to_string(&sp)?;
            let v: Value = serde_json::from_str(&raw).context("parsing seal chain state")?;
            let seq = v
                .get("sequence")
                .and_then(Value::as_u64)
                .ok_or_else(|| anyhow!("corrupt seal state: missing sequence"))?;
            let prev = v
                .get("prev_seal_hash")
                .and_then(Value::as_str)
                .map(String::from);
            (seq, prev)
        } else {
            (0, None)
        };

        Ok(Self {
            key,
            issuer_id: issuer_id.unwrap_or_else(|| "urn:crovia:seal-issuer:causari".to_string()),
            sequence,
            prev_seal_hash,
            state_path: sp,
            log_path: seals_log_path(repo),
        })
    }

    pub fn pubkey_hex(&self) -> String {
        hex::encode(self.key.verifying_key().to_bytes())
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Emit, sign, chain and persist one Seal. Returns the complete Seal.
    pub fn emit(&mut self, subject: SealSubject, generator: SealGenerator) -> Result<Value> {
        let year = chrono::Utc::now().format("%Y").to_string();
        let seal_id = format!("cs_{}_{}", year, random_base32_26()?);

        let mut params = Map::new();
        for (k, v) in &generator.params {
            params.insert(k.clone(), Value::String(v.clone()));
        }

        let mut seal = serde_json::json!({
            "seal_version": SEAL_VERSION,
            "seal_id": seal_id,
            "issuer": {
                "id": self.issuer_id,
                "pubkey": { "alg": "ed25519", "key_hex": self.pubkey_hex() }
            },
            "subject": {
                "input_hash": format!("sha256:{}", sha256_hex(subject.input)),
                "output_hash": format!("sha256:{}", sha256_hex(subject.output)),
                "input_len": subject.input.len(),
                "output_len": subject.output.len(),
                "modality": subject.modality
            },
            "generator": {
                "id": generator.id,
                "version": generator.version,
                "weights_hash": Value::Null,
                "params": Value::Object(params)
            },
            "timestamp": {
                "emitted_at": rfc3339_now_ms(),
                "nonce": random_base32_26()?
            },
            "chain": {
                "prev_seal_hash": self.prev_seal_hash.as_deref(),
                "sequence": self.sequence
            }
        });

        let payload = signing_payload(&seal)?;
        let sig = self.key.sign(&payload);
        seal.as_object_mut().unwrap().insert(
            "signature".to_string(),
            serde_json::json!({
                "alg": "ed25519",
                "canon": "csc-1",
                "domain": "CROVIA-SEAL-v1",
                "payload_hash_alg": "sha256",
                "sig_hex": hex::encode(sig.to_bytes())
            }),
        );

        // Advance the chain: next seal links to SHA256(P(this)).
        self.prev_seal_hash = Some(format!("sha256:{}", sha256_hex(&payload)));
        self.sequence += 1;

        std::fs::create_dir_all(self.state_path.parent().unwrap())?;
        std::fs::write(
            &self.state_path,
            serde_json::to_string_pretty(&serde_json::json!({
                "sequence": self.sequence,
                "prev_seal_hash": self.prev_seal_hash
            }))?,
        )?;
        crate::capture::append_jsonl(&self.log_path, &seal)?;

        Ok(seal)
    }
}

// ---------------------------------------------------------------------------
// Verifier (fail-closed, Section 1.2)
// ---------------------------------------------------------------------------

const KNOWN_TOP_LEVEL: &[&str] = &[
    "seal_version",
    "seal_id",
    "issuer",
    "subject",
    "generator",
    "timestamp",
    "chain",
    "checks",
    "anchor",
    "signature",
    "witnesses",
];

/// Verify a single Seal offline: structure, canonical payload, issuer
/// signature and (if present) every witness signature. Fail-closed.
pub fn verify_seal(seal: &Value) -> Result<()> {
    let obj = seal
        .as_object()
        .ok_or_else(|| anyhow!("seal must be a JSON object"))?;

    for key in obj.keys() {
        if !KNOWN_TOP_LEVEL.contains(&key.as_str()) {
            bail!("unknown top-level field '{}' (fail-closed)", key);
        }
    }
    for required in ["seal_version", "seal_id", "issuer", "subject", "generator", "timestamp", "chain", "signature"] {
        if !obj.contains_key(required) {
            bail!("missing required field '{}'", required);
        }
    }
    if obj["seal_version"].as_str() != Some(SEAL_VERSION) {
        bail!("unsupported seal_version (expected {})", SEAL_VERSION);
    }

    let id = obj["seal_id"]
        .as_str()
        .ok_or_else(|| anyhow!("seal_id must be a string"))?;
    validate_seal_id(id)?;

    let sig_obj = obj["signature"]
        .as_object()
        .ok_or_else(|| anyhow!("signature must be an object"))?;
    if sig_obj.get("alg").and_then(Value::as_str) != Some("ed25519") {
        bail!("unsupported signature.alg");
    }
    if sig_obj.get("canon").and_then(Value::as_str) != Some("csc-1") {
        bail!("unsupported signature.canon");
    }
    if sig_obj.get("domain").and_then(Value::as_str) != Some("CROVIA-SEAL-v1") {
        bail!("wrong signature.domain");
    }

    let key_hex = obj["issuer"]
        .get("pubkey")
        .and_then(|p| p.get("key_hex"))
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("missing issuer.pubkey.key_hex"))?;
    let sig_hex = sig_obj
        .get("sig_hex")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("missing signature.sig_hex"))?;

    let payload = signing_payload(seal)?;
    verify_sig(key_hex, sig_hex, &payload).context("issuer signature invalid")?;

    if let Some(witnesses) = obj.get("witnesses") {
        let arr = witnesses
            .as_array()
            .ok_or_else(|| anyhow!("witnesses must be an array"))?;
        for (i, w) in arr.iter().enumerate() {
            let wkey = w
                .get("pubkey")
                .and_then(|p| p.get("key_hex"))
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("witness {} missing pubkey", i))?;
            let wsig = w
                .get("sig_hex")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("witness {} missing sig_hex", i))?;
            verify_sig(wkey, wsig, &payload)
                .with_context(|| format!("witness {} signature invalid (fail-closed)", i))?;
        }
    }
    Ok(())
}

fn validate_seal_id(id: &str) -> Result<()> {
    // ^cs_[0-9]{4}_[A-Z2-7]{26}$
    let ok = id.len() == 3 + 4 + 1 + 26
        && id.starts_with("cs_")
        && id.as_bytes()[3..7].iter().all(u8::is_ascii_digit)
        && id.as_bytes()[7] == b'_'
        && id.as_bytes()[8..]
            .iter()
            .all(|b| b.is_ascii_uppercase() || (b'2'..=b'7').contains(b));
    if !ok {
        bail!("malformed seal_id '{}'", id);
    }
    Ok(())
}

fn verify_sig(key_hex: &str, sig_hex: &str, payload: &[u8]) -> Result<()> {
    let pk_bytes: [u8; 32] = hex::decode(key_hex)
        .context("decoding pubkey hex")?
        .try_into()
        .map_err(|_| anyhow!("pubkey must be 32 bytes"))?;
    let sig_bytes: [u8; 64] = hex::decode(sig_hex)
        .context("decoding signature hex")?
        .try_into()
        .map_err(|_| anyhow!("signature must be 64 bytes"))?;
    let pk = VerifyingKey::from_bytes(&pk_bytes).context("invalid Ed25519 pubkey")?;
    pk.verify(payload, &Signature::from_bytes(&sig_bytes))
        .map_err(|_| anyhow!("Ed25519 verification failed"))
}

/// Verify the full local chain in `seals.jsonl`: every signature valid,
/// sequences contiguous from 0, every `prev_seal_hash` linking correctly.
/// Returns the number of verified seals.
pub fn verify_chain(repo: &Repo) -> Result<usize> {
    let path = seals_log_path(repo);
    if !path.exists() {
        return Ok(0);
    }
    let raw = std::fs::read_to_string(&path)?;
    let mut expected_seq: u64 = 0;
    let mut expected_prev: Option<String> = None;
    let mut count = 0usize;
    for (lineno, line) in raw.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let seal: Value = serde_json::from_str(line)
            .with_context(|| format!("seals.jsonl line {}: invalid JSON", lineno + 1))?;
        verify_seal(&seal).with_context(|| format!("seals.jsonl line {}", lineno + 1))?;

        let seq = seal["chain"]["sequence"]
            .as_u64()
            .ok_or_else(|| anyhow!("line {}: missing chain.sequence", lineno + 1))?;
        if seq != expected_seq {
            bail!(
                "chain gap or fork at line {}: expected sequence {}, found {}",
                lineno + 1,
                expected_seq,
                seq
            );
        }
        let prev = seal["chain"]["prev_seal_hash"].as_str().map(String::from);
        if prev != expected_prev {
            bail!("chain link broken at line {} (prev_seal_hash mismatch)", lineno + 1);
        }

        let payload = signing_payload(&seal)?;
        expected_prev = Some(format!("sha256:{}", sha256_hex(&payload)));
        expected_seq += 1;
        count += 1;
    }
    Ok(count)
}

// ---------------------------------------------------------------------------
// Tests: normative conformance vectors from croviatrust/crovia-seal
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const VEC_GENESIS: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/vectors/seal_001_genesis.json"
    ));
    const VEC_GENESIS_PAYLOAD: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/vectors/seal_001_genesis.payload.hex"
    ));
    const VEC_CHAINED: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/vectors/seal_002_chained.json"
    ));
    const VEC_UTF8: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/vectors/seal_010_utf8_content.json"
    ));
    const VEC_CANONICAL: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/vectors/canonical_cases.json"
    ));

    #[test]
    fn csc1_matches_all_normative_canonical_cases() {
        let doc: Value = serde_json::from_str(VEC_CANONICAL).unwrap();
        for case in doc["cases"].as_array().unwrap() {
            let name = case["name"].as_str().unwrap();
            let got = csc1_serialize(&case["input"]).unwrap();
            let expected = hex::decode(case["expected_hex"].as_str().unwrap()).unwrap();
            assert_eq!(got, expected, "canonical case '{}' diverges", name);
        }
    }

    #[test]
    fn csc1_rejects_floats() {
        assert!(csc1_serialize(&serde_json::json!(0.7)).is_err());
        assert!(csc1_serialize(&serde_json::json!({"t": 1.5})).is_err());
    }

    #[test]
    fn payload_bytes_match_genesis_vector() {
        let seal: Value = serde_json::from_str(VEC_GENESIS).unwrap();
        let payload = signing_payload(&seal).unwrap();
        assert_eq!(hex::encode(&payload), VEC_GENESIS_PAYLOAD.trim());
    }

    #[test]
    fn normative_vectors_verify() {
        for (name, raw) in [
            ("genesis", VEC_GENESIS),
            ("chained", VEC_CHAINED),
            ("utf8", VEC_UTF8),
        ] {
            let seal: Value = serde_json::from_str(raw).unwrap();
            verify_seal(&seal).unwrap_or_else(|e| panic!("vector '{}' failed: {:#}", name, e));
        }
    }

    #[test]
    fn tampered_vector_fails() {
        let mut seal: Value = serde_json::from_str(VEC_GENESIS).unwrap();
        seal["subject"]["input_len"] = serde_json::json!(31);
        assert!(verify_seal(&seal).is_err());
    }

    #[test]
    fn unknown_top_level_field_fails_closed() {
        let mut seal: Value = serde_json::from_str(VEC_GENESIS).unwrap();
        seal["extra"] = serde_json::json!("nope");
        assert!(verify_seal(&seal).is_err());
    }

    #[test]
    fn emit_verify_and_chain_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        let mut issuer = SealIssuer::load_or_create(&repo, None).unwrap();

        for i in 0..3 {
            let input = format!("request {}", i);
            let output = format!("response {}", i);
            let seal = issuer
                .emit(
                    SealSubject {
                        input: input.as_bytes(),
                        output: output.as_bytes(),
                        modality: "text",
                    },
                    SealGenerator {
                        id: "openai/gpt-4o",
                        version: None,
                        params: vec![("temperature".into(), "0.7".into())],
                    },
                )
                .unwrap();
            verify_seal(&seal).unwrap();
            assert_eq!(seal["chain"]["sequence"].as_u64().unwrap(), i);
        }
        assert_eq!(verify_chain(&repo).unwrap(), 3);

        // Issuer state survives a reload: chain continues, no fork.
        let mut issuer2 = SealIssuer::load_or_create(&repo, None).unwrap();
        assert_eq!(issuer2.sequence(), 3);
        let seal = issuer2
            .emit(
                SealSubject {
                    input: b"again",
                    output: b"again",
                    modality: "text",
                },
                SealGenerator {
                    id: "anthropic/claude",
                    version: Some("2026-01"),
                    params: vec![],
                },
            )
            .unwrap();
        assert_eq!(seal["chain"]["sequence"].as_u64().unwrap(), 3);
        assert_eq!(verify_chain(&repo).unwrap(), 4);
    }

    #[test]
    fn base32_is_rfc4648() {
        // RFC 4648 test vector: "foobar" -> MZXW6YTBOI (no padding)
        assert_eq!(base32_nopad(b"foobar"), "MZXW6YTBOI");
        assert_eq!(base32_nopad(&[0u8; 16]).len(), 26);
    }
}
