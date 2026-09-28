//! PNX — Proof of Non-Exfiltration, TACET profile `crovia.pnx.v1`.
//!
//! An egress witness (here: `re proxy --pnx`) sees every request body an
//! agent sends to its model provider. It fingerprints each body with a
//! per-run salt (winnowing, Schleimer–Wilkerson–Aiken 2003), commits the
//! fingerprints as keys of a depth-256 sparse Merkle map (TACET SPEC §5)
//! and signs a **run sheet** carrying the root. Afterwards the operator can
//! prove, to anyone and offline, that a set of protected assets never shared
//! a substring of `THRESHOLD` bytes with that traffic — or, when one did,
//! hand over a signed record of exposure. Neither the sheet nor the proof
//! contains traffic bytes or asset bytes.
//!
//! This module is the protocol only: fingerprints, map, sheet, proof,
//! verification. Its output is byte-identical to the Python reference in
//! `crovia-tacet` (`tacet/egress.py`, `tacet/smt.py`): same roots for the
//! same key set, same compact path encoding, same signed payload. That is
//! asserted by the vectors under `tests/vectors/pnx/` and, when the
//! reference is importable, by live cross-checks in the tests below.
//!
//! What a verified proof does and does not say is spelled out in
//! `docs/pnx.md`; the short version: nothing about bytes the witness did not
//! see, nothing about encodings it did not normalise, nothing about assets
//! shorter than `K_GRAM` bytes.

use anyhow::{Context, Result, anyhow, bail};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::LazyLock;

use crate::pnx_reach::{Policy, ReachVerifyResult, verify_reach};
use crate::seal::csc1_serialize;

pub const PROFILE: &str = "crovia.pnx.v1";
pub const PROOF_VERSION: &str = "crovia.pnx.proof.v1";
pub const K_GRAM: usize = 32;
pub const WINDOW: usize = 16;
/// Shared substrings at least this long are always detected.
pub const THRESHOLD: usize = K_GRAM + WINDOW - 1;

pub const DOMAIN_FINGERPRINT: &[u8] = b"CROVIA-PNX-FP-v1\n";
/// The signed payload is this domain (newline included) followed by the
/// CSC-1 encoding of the sheet without its `signature` member. The sheet's
/// `signature.domain` field carries the domain without the newline.
pub const DOMAIN_SHEET: &[u8] = b"CROVIA-PNX-SHEET-v1\n";
pub const DOMAIN_LEAF_VALUE: &[u8] = b"CROVIA-PNX-PRESENT-v1\n";

pub const NORMALIZE_JSON_STRINGS: &str = "json-strings-v1";

pub const VERDICT_ABSENT: &str = "absent";
pub const VERDICT_ABSENT_PARTIAL: &str = "absent-partial";
pub const VERDICT_PRESENT: &str = "present";
pub const VERDICT_UNDETECTABLE: &str = "undetectable";
pub const VERDICT_MIXED: &str = "mixed";

const DOMAIN_EMPTY: &[u8] = b"TACET-EMPTY-v1\n";
const DOMAIN_LEAF: &[u8] = b"TACET-LEAF-v1\n";
const DOMAIN_NODE: &[u8] = b"TACET-NODE-v1\n";
pub const DEPTH: usize = 256;

pub type Hash = [u8; 32];

fn sha256(parts: &[&[u8]]) -> Hash {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

pub fn prefixed(digest: &Hash) -> String {
    format!("sha256:{}", hex::encode(digest))
}

pub fn unprefixed(value: &Value) -> Result<Hash> {
    let s = value
        .as_str()
        .ok_or_else(|| anyhow!("expected 'sha256:<64 hex>', got {value}"))?;
    let hex_part = s
        .strip_prefix("sha256:")
        .filter(|h| h.len() == 64)
        .ok_or_else(|| anyhow!("expected 'sha256:<64 hex>', got {s:?}"))?;
    hash_from_hex(hex_part)
}

fn hash_from_hex(s: &str) -> Result<Hash> {
    let bytes = hex::decode(s).with_context(|| format!("invalid hex {s:?}"))?;
    bytes
        .try_into()
        .map_err(|_| anyhow!("expected 32 bytes of hex, got {s:?}"))
}

// ---------------------------------------------------------------------------
// TACET sparse Merkle map (SPEC §5): domain-separated hashing
// ---------------------------------------------------------------------------

/// `EMPTY[h]` is the hash of an empty subtree of height `h`; `EMPTY[256]` is
/// the root of an empty map.
static EMPTY: LazyLock<Vec<Hash>> = LazyLock::new(|| {
    let mut table = vec![sha256(&[DOMAIN_EMPTY])];
    for _ in 0..DEPTH {
        let last = *table.last().expect("non-empty");
        table.push(sha256(&[DOMAIN_NODE, &last, &last]));
    }
    table
});

/// The constant leaf value of every fingerprint present in a run map.
static PRESENT: LazyLock<Hash> = LazyLock::new(|| sha256(&[DOMAIN_LEAF_VALUE]));

fn leaf_hash(key: &Hash, value: &Hash) -> Hash {
    sha256(&[DOMAIN_LEAF, key, value])
}

fn node_hash(left: &Hash, right: &Hash) -> Hash {
    sha256(&[DOMAIN_NODE, left, right])
}

/// Bit of `key` consumed at path position `position` (0 = MSB, at the root).
fn key_bit(key: &Hash, position: usize) -> u8 {
    (key[position / 8] >> (7 - position % 8)) & 1
}

// ---------------------------------------------------------------------------
// Fingerprints
// ---------------------------------------------------------------------------

/// Winnowing parameters. `crovia.pnx.v1` fixes them to 32/16; a verifier
/// recomputes with whatever a sheet declares, so they travel explicitly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Params {
    pub k_gram: usize,
    pub window: usize,
}

impl Params {
    pub const DEFAULT: Params = Params {
        k_gram: K_GRAM,
        window: WINDOW,
    };

    pub fn threshold(&self) -> usize {
        self.k_gram + self.window - 1
    }
}

impl Default for Params {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Salted hash of every k-gram of `data`, in order; empty when `data` is
/// shorter than `k`.
pub fn kgram_hashes(data: &[u8], salt: &[u8; 16], k: usize) -> Vec<Hash> {
    if k == 0 || data.len() < k {
        return Vec::new();
    }
    let mut base = Sha256::new();
    base.update(DOMAIN_FINGERPRINT);
    base.update(salt);
    data.windows(k)
        .map(|gram| {
            let mut h = base.clone();
            h.update(gram);
            h.finalize().into()
        })
        .collect()
}

/// The minimum of every window of `w` consecutive hashes. The reference
/// picks the rightmost minimum on ties; since the result is a *set* of
/// values, which occurrence is picked never changes it.
pub fn winnow(hashes: &[Hash], w: usize) -> BTreeSet<Hash> {
    let mut out = BTreeSet::new();
    if hashes.is_empty() || w == 0 {
        return out;
    }
    if hashes.len() < w {
        out.insert(*hashes.iter().min().expect("non-empty"));
        return out;
    }
    for window in hashes.windows(w) {
        out.insert(*window.iter().min().expect("non-empty window"));
    }
    out
}

/// Winnowed fingerprints of `data`; empty when it is shorter than `k`.
pub fn fingerprints(data: &[u8], salt: &[u8; 16], p: Params) -> BTreeSet<Hash> {
    winnow(&kgram_hashes(data, salt, p.k_gram), p.window)
}

/// How well an asset of a given length can be detected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detection {
    /// `len >= threshold`: a shared substring of `threshold` bytes always
    /// yields a common fingerprint.
    Guaranteed,
    /// `k_gram <= len < threshold`: every k-gram hash is checked; detection
    /// is possible, not guaranteed.
    Partial,
    /// `len < k_gram`: no k-gram can be formed. Never counted as clean.
    Undetectable,
}

impl Detection {
    pub fn as_str(self) -> &'static str {
        match self {
            Detection::Guaranteed => "guaranteed",
            Detection::Partial => "partial",
            Detection::Undetectable => "undetectable",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "guaranteed" => Some(Detection::Guaranteed),
            "partial" => Some(Detection::Partial),
            "undetectable" => Some(Detection::Undetectable),
            _ => None,
        }
    }

    pub fn for_len(len: usize, p: Params) -> Self {
        if len < p.k_gram {
            Detection::Undetectable
        } else if len >= p.threshold() {
            Detection::Guaranteed
        } else {
            Detection::Partial
        }
    }
}

/// The fingerprints to check for an asset and the detection class they
/// carry, sorted.
pub fn asset_fingerprints(asset: &[u8], salt: &[u8; 16], p: Params) -> (Detection, Vec<Hash>) {
    let class = Detection::for_len(asset.len(), p);
    match class {
        Detection::Undetectable => (class, Vec::new()),
        Detection::Guaranteed => {
            let fps = winnow(&kgram_hashes(asset, salt, p.k_gram), p.window);
            (class, fps.into_iter().collect())
        }
        Detection::Partial => {
            let set: BTreeSet<Hash> = kgram_hashes(asset, salt, p.k_gram).into_iter().collect();
            (class, set.into_iter().collect())
        }
    }
}

/// `json-strings-v1`: the decoded string values of a JSON body that can
/// hold at least one k-gram, in document order. A file quoted inside an LLM
/// request arrives with its newlines and quotes escaped, so its raw bytes
/// never form a 47-byte run; the decoded strings restore the guarantee.
/// Non-JSON (or non-UTF-8) bodies yield nothing.
pub fn json_strings(body: &[u8], min_len: usize) -> Vec<Vec<u8>> {
    let Ok(text) = std::str::from_utf8(body) else {
        return Vec::new();
    };
    let Ok(doc) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect_strings(&doc, min_len, &mut out);
    out
}

fn collect_strings(v: &Value, min_len: usize, out: &mut Vec<Vec<u8>>) {
    match v {
        Value::String(s) => {
            if s.len() >= min_len {
                out.push(s.as_bytes().to_vec());
            }
        }
        Value::Array(items) => items.iter().for_each(|x| collect_strings(x, min_len, out)),
        Value::Object(map) => map.values().for_each(|x| collect_strings(x, min_len, out)),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Sparse Merkle map of fingerprints
// ---------------------------------------------------------------------------

/// Sibling path with default (empty) siblings elided. Bit `h` of `bitmap`
/// is set iff the sibling of height `h` is listed in `siblings`, which holds
/// present siblings in increasing height (`siblings[0]` is closest to the
/// leaf). Serialised as `{"bitmap": <64 hex>, "siblings": [<64 hex>...]}`,
/// the bitmap being a 256-bit big-endian integer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompactPath {
    pub bitmap: [u8; 32],
    pub siblings: Vec<Hash>,
}

fn bitmap_bit(bitmap: &[u8; 32], h: usize) -> bool {
    (bitmap[31 - h / 8] >> (h % 8)) & 1 == 1
}

impl CompactPath {
    fn from_full(full: &[Hash]) -> Self {
        debug_assert_eq!(full.len(), DEPTH);
        let mut bitmap = [0u8; 32];
        let mut siblings = Vec::new();
        for (h, sib) in full.iter().enumerate() {
            if *sib != EMPTY[h] {
                bitmap[31 - h / 8] |= 1 << (h % 8);
                siblings.push(*sib);
            }
        }
        Self { bitmap, siblings }
    }

    fn to_full(&self) -> Result<Vec<Hash>> {
        let mut it = self.siblings.iter();
        let mut full = Vec::with_capacity(DEPTH);
        for h in 0..DEPTH {
            if bitmap_bit(&self.bitmap, h) {
                full.push(
                    *it.next()
                        .ok_or_else(|| anyhow!("path lists fewer siblings than its bitmap"))?,
                );
            } else {
                full.push(EMPTY[h]);
            }
        }
        if it.next().is_some() {
            bail!("path lists more siblings than its bitmap");
        }
        Ok(full)
    }

    pub fn to_json(&self) -> Value {
        json!({
            "bitmap": hex::encode(self.bitmap),
            "siblings": self.siblings.iter().map(hex::encode).collect::<Vec<_>>(),
        })
    }

    pub fn from_json(v: &Value) -> Result<Self> {
        let bitmap_hex = v
            .get("bitmap")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("path.bitmap must be a hex string"))?;
        let padded = if bitmap_hex.len() % 2 == 1 {
            format!("0{bitmap_hex}")
        } else {
            bitmap_hex.to_string()
        };
        let raw = hex::decode(&padded).context("path.bitmap is not hex")?;
        if raw.len() > 32 {
            bail!("path.bitmap exceeds 256 bits");
        }
        let mut bitmap = [0u8; 32];
        bitmap[32 - raw.len()..].copy_from_slice(&raw);
        let siblings = v
            .get("siblings")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("path.siblings must be an array"))?
            .iter()
            .map(|s| {
                s.as_str()
                    .ok_or_else(|| anyhow!("sibling must be a hex string"))
                    .and_then(hash_from_hex)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { bitmap, siblings })
    }
}

/// Fold a leaf hash up to the root along `key`, leaf-side siblings first.
fn root_from_path(key: &Hash, leaf: Hash, siblings: &[Hash]) -> Hash {
    let mut node = leaf;
    for (h, sib) in siblings.iter().enumerate() {
        let position = DEPTH - 1 - h;
        node = if key_bit(key, position) == 1 {
            node_hash(sib, &node)
        } else {
            node_hash(&node, sib)
        };
    }
    node
}

pub fn verify_inclusion(root: &Hash, key: &Hash, path: &CompactPath) -> bool {
    match path.to_full() {
        Ok(full) => root_from_path(key, leaf_hash(key, &PRESENT), &full) == *root,
        Err(_) => false,
    }
}

pub fn verify_non_inclusion(root: &Hash, key: &Hash, path: &CompactPath) -> bool {
    match path.to_full() {
        Ok(full) => root_from_path(key, EMPTY[0], &full) == *root,
        Err(_) => false,
    }
}

/// Hash of the subtree at `depth` holding `keys[lo..hi]` (sorted keys that
/// share the first `depth` bits). Nodes with two or more keys are memoised;
/// a node with one key is a chain of `256 - depth` hashes and is recomputed
/// on demand, which keeps the cache proportional to the key count rather
/// than to the key count times the depth.
fn subtree(
    keys: &[Hash],
    cache: &mut HashMap<(u16, u32), Hash>,
    depth: usize,
    lo: usize,
    hi: usize,
) -> Hash {
    let height = DEPTH - depth;
    if lo == hi {
        return EMPTY[height];
    }
    if hi - lo == 1 {
        return single_key_chain(&keys[lo], depth);
    }
    let cache_key = (depth as u16, lo as u32);
    if let Some(h) = cache.get(&cache_key) {
        return *h;
    }
    let mid = lo + keys[lo..hi].partition_point(|k| key_bit(k, depth) == 0);
    let left = subtree(keys, cache, depth + 1, lo, mid);
    let right = subtree(keys, cache, depth + 1, mid, hi);
    let h = node_hash(&left, &right);
    cache.insert(cache_key, h);
    h
}

/// Subtree hash at `depth` when `key` is the only key below it.
fn single_key_chain(key: &Hash, depth: usize) -> Hash {
    let mut node = leaf_hash(key, &PRESENT);
    for h in 0..(DEPTH - depth) {
        let position = DEPTH - 1 - h;
        node = if key_bit(key, position) == 1 {
            node_hash(&EMPTY[h], &node)
        } else {
            node_hash(&node, &EMPTY[h])
        };
    }
    node
}

/// In-memory depth-256 sparse Merkle map whose every leaf holds the
/// constant `PRESENT` value: a committed set of fingerprints.
#[derive(Clone, Default)]
pub struct SparseMerkleMap {
    keys: BTreeSet<Hash>,
    sorted: Vec<Hash>,
    cache: HashMap<(u16, u32), Hash>,
    dirty: bool,
}

impl SparseMerkleMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn contains(&self, key: &Hash) -> bool {
        self.keys.contains(key)
    }

    /// Insert a key; returns whether it was new.
    pub fn insert(&mut self, key: Hash) -> bool {
        let new = self.keys.insert(key);
        if new {
            self.dirty = true;
        }
        new
    }

    fn prepare(&mut self) {
        if self.dirty || (self.sorted.len() != self.keys.len()) {
            self.sorted = self.keys.iter().copied().collect();
            self.cache.clear();
            self.dirty = false;
        }
    }

    pub fn root(&mut self) -> Hash {
        self.prepare();
        let n = self.sorted.len();
        subtree(&self.sorted, &mut self.cache, 0, 0, n)
    }

    /// Sibling path for `key`: an inclusion proof when the key is present,
    /// a non-inclusion proof otherwise.
    pub fn prove(&mut self, key: &Hash) -> CompactPath {
        self.prepare();
        let keys = &self.sorted;
        let mut full = vec![[0u8; 32]; DEPTH];
        let (mut lo, mut hi) = (0usize, keys.len());
        for depth in 0..DEPTH {
            let bit = key_bit(key, depth);
            let mid = lo + keys[lo..hi].partition_point(|k| key_bit(k, depth) == 0);
            let (same, other) = if bit == 0 {
                ((lo, mid), (mid, hi))
            } else {
                ((mid, hi), (lo, mid))
            };
            full[DEPTH - 1 - depth] = subtree(keys, &mut self.cache, depth + 1, other.0, other.1);
            (lo, hi) = same;
        }
        CompactPath::from_full(&full)
    }
}

// ---------------------------------------------------------------------------
// Witness: bodies in, signed run sheet and proofs out
// ---------------------------------------------------------------------------

/// Accumulates the fingerprints of one run's outbound bodies.
pub struct Witness {
    pub run_id: String,
    pub salt: [u8; 16],
    pub params: Params,
    pub map: SparseMerkleMap,
    pub bodies: u64,
    pub bytes: u64,
    pub first_at: Option<String>,
    pub last_at: Option<String>,
    /// Normalisation layers applied, sorted. Empty when none.
    pub normalization: Vec<String>,
    /// The reach record (PNX.md §4a): where the run connected, under which
    /// policy. Optional; signed with the rest of the sheet when present.
    pub reach: Option<Value>,
}

impl Witness {
    /// A fresh witness applying `json-strings-v1`.
    pub fn new(run_id: &str, salt: [u8; 16]) -> Self {
        Self {
            run_id: run_id.to_string(),
            salt,
            params: Params::DEFAULT,
            map: SparseMerkleMap::new(),
            bodies: 0,
            bytes: 0,
            first_at: None,
            last_at: None,
            normalization: vec![NORMALIZE_JSON_STRINGS.to_string()],
            reach: None,
        }
    }

    fn add(&mut self, data: &[u8], added: &mut Vec<Hash>) {
        for fp in fingerprints(data, &self.salt, self.params) {
            if self.map.insert(fp) {
                added.push(fp);
            }
        }
    }

    /// Record one outbound body observed at RFC 3339 time `at`. The raw
    /// bytes are always fingerprinted; with `json-strings-v1` on, every
    /// decoded string value of a JSON body is fingerprinted as well.
    /// `bodies` and `bytes` count the raw body only. Returns the
    /// fingerprints that were new to the map.
    pub fn ingest(&mut self, body: &[u8], at: &str) -> Vec<Hash> {
        let mut added = Vec::new();
        self.add(body, &mut added);
        if self
            .normalization
            .iter()
            .any(|n| n == NORMALIZE_JSON_STRINGS)
        {
            for derived in json_strings(body, self.params.k_gram) {
                self.add(&derived, &mut added);
            }
        }
        self.bodies += 1;
        self.bytes += body.len() as u64;
        if self.first_at.is_none() {
            self.first_at = Some(at.to_string());
        }
        self.last_at = Some(at.to_string());
        added
    }

    pub fn root(&mut self) -> Hash {
        self.map.root()
    }

    /// The signed run sheet: the one object that has to leave the machine.
    pub fn sheet(&mut self, key: &SigningKey, witness_id: &str, closed_at: &str) -> Result<Value> {
        let root = self.root();
        let mut normalization = self.normalization.clone();
        normalization.sort();
        let mut sheet = json!({
            "profile": PROFILE,
            "run_id": self.run_id,
            "salt_hex": hex::encode(self.salt),
            "params": {
                "k_gram": self.params.k_gram,
                "window": self.params.window,
                "threshold": self.params.threshold(),
                "hash": "sha256",
            },
            "egress": {
                "bodies": self.bodies,
                "bytes": self.bytes,
                "first_at": self.first_at,
                "last_at": self.last_at,
            },
            "normalization": normalization,
            "fingerprints": self.map.len(),
            "root": prefixed(&root),
            "closed_at": closed_at,
            "witness": {
                "id": witness_id,
                "pubkey": {"alg": "ed25519", "key_hex": hex::encode(key.verifying_key().to_bytes())},
            },
        });
        if let Some(reach) = &self.reach {
            sheet["reach"] = reach.clone();
        }
        let sig = key.sign(&sheet_payload(&sheet)?);
        sheet["signature"] = json!({
            "alg": "ed25519",
            "domain": "CROVIA-PNX-SHEET-v1",
            "sig_hex": hex::encode(sig.to_bytes()),
        });
        Ok(sheet)
    }

    /// Proof of Non-Exfiltration for labelled assets against `sheet`, which
    /// must be this witness's own (same root, same salt).
    pub fn prove(&mut self, sheet: &Value, assets: &[(String, Vec<u8>)]) -> Result<Value> {
        let root = self.root();
        if unprefixed(&sheet["root"])? != root {
            bail!("sheet root does not match the witness map");
        }
        let salt_hex = sheet["salt_hex"].as_str().unwrap_or("");
        if salt_hex != hex::encode(self.salt) {
            bail!("sheet salt does not match the witness");
        }
        let mut seen = BTreeSet::new();
        let mut out_assets = Vec::new();
        for (label, data) in assets {
            if !seen.insert(label.clone()) {
                bail!("duplicate asset label {label:?}");
            }
            let (class, fps) = asset_fingerprints(data, &self.salt, self.params);
            let mut present = 0usize;
            let mut entries = Vec::new();
            for fp in &fps {
                let path = self.map.prove(fp);
                let hit = self.map.contains(fp);
                present += usize::from(hit);
                entries
                    .push(json!({"key": hex::encode(fp), "present": hit, "path": path.to_json()}));
            }
            let verdict = verdict_for(class, present > 0);
            out_assets.push(json!({
                "label": label,
                "asset_len": data.len(),
                "detection": class.as_str(),
                "asset_sha256": prefixed(&sha256(&[data])),
                "fingerprints": entries,
                "verdict": verdict,
            }));
        }
        let verdicts: Vec<&str> = out_assets
            .iter()
            .map(|a| a["verdict"].as_str().unwrap_or(""))
            .collect();
        Ok(json!({
            "profile": PROFILE,
            "proof_version": PROOF_VERSION,
            "sheet": sheet,
            "assets": out_assets,
            "verdict": overall_verdict(&verdicts),
        }))
    }
}

fn verdict_for(class: Detection, present: bool) -> &'static str {
    match class {
        Detection::Undetectable => VERDICT_UNDETECTABLE,
        _ if present => VERDICT_PRESENT,
        Detection::Guaranteed => VERDICT_ABSENT,
        Detection::Partial => VERDICT_ABSENT_PARTIAL,
    }
}

/// `present` if any asset is present, `absent` if every asset is absent,
/// otherwise `mixed` (including the degenerate empty proof).
fn overall_verdict(verdicts: &[&str]) -> &'static str {
    if verdicts.contains(&VERDICT_PRESENT) {
        VERDICT_PRESENT
    } else if !verdicts.is_empty() && verdicts.iter().all(|v| *v == VERDICT_ABSENT) {
        VERDICT_ABSENT
    } else {
        VERDICT_MIXED
    }
}

/// `DOMAIN_SHEET ‖ CSC-1(sheet \ {signature})`.
pub fn sheet_payload(sheet: &Value) -> Result<Vec<u8>> {
    let obj = sheet
        .as_object()
        .ok_or_else(|| anyhow!("sheet must be a JSON object"))?;
    let mut unsigned: Map<String, Value> = obj.clone();
    unsigned.remove("signature");
    let mut payload = DOMAIN_SHEET.to_vec();
    payload.extend_from_slice(&csc1_serialize(&Value::Object(unsigned))?);
    Ok(payload)
}

// ---------------------------------------------------------------------------
// Verification (PNX.md §6, steps 1–4; offline)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub ok: bool,
    /// The recomputed overall verdict (the proof's stated one until the
    /// sheet has been checked).
    pub verdict: String,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    /// Recomputed per-asset verdicts, in proof order.
    pub assets: Vec<(String, String)>,
    /// Set when the sheet carries a reach record (PNX.md §4a).
    pub reach: Option<ReachVerifyResult>,
}

/// Parameters a sheet declares, validated for consistency.
pub fn sheet_params(sheet: &Value) -> Result<Params> {
    let p = sheet
        .get("params")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("inconsistent params"))?;
    let int = |k: &str| p.get(k).and_then(Value::as_u64).map(|v| v as usize);
    let (Some(k), Some(w), Some(t)) = (int("k_gram"), int("window"), int("threshold")) else {
        bail!("inconsistent params");
    };
    if p.get("hash").and_then(Value::as_str) != Some("sha256") || k == 0 || w == 0 || t != k + w - 1
    {
        bail!("inconsistent params");
    }
    Ok(Params {
        k_gram: k,
        window: w,
    })
}

pub fn sheet_salt(sheet: &Value) -> Result<[u8; 16]> {
    let raw = hex::decode(sheet.get("salt_hex").and_then(Value::as_str).unwrap_or("!"))
        .context("malformed sheet: salt_hex is not hex")?;
    raw.try_into()
        .map_err(|_| anyhow!("malformed sheet: salt must be 16 bytes"))
}

/// Step 1: profile, parameter consistency, known normalisation layers,
/// Ed25519 signature over the canonical unsigned sheet.
pub fn verify_sheet(sheet: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    if sheet.get("profile").and_then(Value::as_str) != Some(PROFILE) {
        errors.push(format!(
            "unknown profile {}",
            sheet.get("profile").unwrap_or(&Value::Null)
        ));
    }
    if let Err(e) = unprefixed(sheet.get("root").unwrap_or(&Value::Null)) {
        errors.push(format!("malformed sheet: {e}"));
        return errors;
    }
    if let Err(e) = sheet_salt(sheet) {
        errors.push(e.to_string());
        return errors;
    }
    if let Err(e) = sheet_params(sheet) {
        errors.push(e.to_string());
    }
    let norm = sheet
        .get("normalization")
        .unwrap_or(&Value::Array(vec![]))
        .clone();
    let known = norm.as_array().is_some_and(|layers| {
        layers
            .iter()
            .all(|n| n.as_str() == Some(NORMALIZE_JSON_STRINGS))
    });
    if !known {
        errors.push(format!("unknown normalization layers {norm}"));
    }
    let key_hex = sheet
        .pointer("/witness/pubkey/key_hex")
        .and_then(Value::as_str)
        .unwrap_or("");
    let sig_hex = sheet
        .pointer("/signature/sig_hex")
        .and_then(Value::as_str)
        .unwrap_or("");
    let ok = sheet_payload(sheet)
        .map(|payload| verify_signature(key_hex, &payload, sig_hex))
        .unwrap_or(false);
    if !ok {
        errors.push("witness signature invalid".to_string());
    }
    if let Some(reach) = sheet.get("reach") {
        if !reach.is_object() {
            errors.push("reach must be an object".to_string());
        } else {
            let salt = sheet_salt(sheet).unwrap_or([0u8; 16]);
            errors.extend(
                verify_reach(reach, &salt, None, &[])
                    .errors
                    .into_iter()
                    .map(|e| format!("reach: {e}")),
            );
        }
    }
    errors
}

fn verify_signature(key_hex: &str, message: &[u8], sig_hex: &str) -> bool {
    let (Ok(pk), Ok(sig)) = (hex::decode(key_hex), hex::decode(sig_hex)) else {
        return false;
    };
    let (Ok(pk), Ok(sig)) = (<[u8; 32]>::try_from(pk), <[u8; 64]>::try_from(sig)) else {
        return false;
    };
    VerifyingKey::from_bytes(&pk)
        .map(|vk| vk.verify(message, &Signature::from_bytes(&sig)).is_ok())
        .unwrap_or(false)
}

/// Verify a PNX proof offline (steps 1–4 of PNX.md §6).
///
/// With `assets` (label → bytes) every fingerprint set is recomputed, so
/// the prover cannot substitute keys; every asset the proof lists must then
/// be supplied. Without them the paths are verified for the keys the proof
/// lists, which proves non-inclusion of *those keys* only; the result
/// carries a warning saying so.
#[cfg(test)]
pub fn verify_proof(proof: &Value, assets: Option<&BTreeMap<String, Vec<u8>>>) -> VerifyResult {
    verify_proof_with(proof, assets, None, &[])
}

/// [`verify_proof`], plus the reach record (PNX.md §6 step 1b): with the
/// `policy` document its hash is recomputed and every destination matched
/// against the rules; `names` are hosts to look up under salted disclosure.
pub fn verify_proof_with(
    proof: &Value,
    assets: Option<&BTreeMap<String, Vec<u8>>>,
    policy: Option<&Policy>,
    names: &[String],
) -> VerifyResult {
    let mut res = VerifyResult {
        ok: true,
        verdict: proof
            .get("verdict")
            .and_then(Value::as_str)
            .unwrap_or("?")
            .to_string(),
        errors: Vec::new(),
        warnings: Vec::new(),
        assets: Vec::new(),
        reach: None,
    };
    let sheet = proof.get("sheet").cloned().unwrap_or(Value::Null);
    res.errors.extend(verify_sheet(&sheet));
    if !res.errors.is_empty() {
        res.ok = false;
        return res;
    }
    let root = unprefixed(&sheet["root"]).expect("checked by verify_sheet");
    let salt = sheet_salt(&sheet).expect("checked by verify_sheet");
    let params = sheet_params(&sheet).expect("checked by verify_sheet");
    if assets.is_none() {
        res.warnings.push(
            "assets not supplied: fingerprints taken from the proof, not recomputed".to_string(),
        );
    }
    if let Some(reach) = sheet.get("reach").filter(|r| r.is_object()) {
        // Structure was checked by verify_sheet; this is the policy conformance.
        let r = verify_reach(reach, &salt, policy, names);
        res.errors
            .extend(r.errors.iter().map(|e| format!("reach: {e}")));
        res.warnings
            .extend(r.warnings.iter().map(|w| format!("reach: {w}")));
        res.reach = Some(r);
        if !res.errors.is_empty() {
            res.ok = false;
            return res;
        }
    } else if policy.is_some() {
        res.warnings
            .push("policy document supplied but the sheet carries no reach record".to_string());
    }

    let empty = Vec::new();
    for a in proof
        .get("assets")
        .and_then(Value::as_array)
        .unwrap_or(&empty)
    {
        let label = a
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or("?")
            .to_string();
        let listed = a
            .get("fingerprints")
            .and_then(Value::as_array)
            .unwrap_or(&empty);
        let stated_class = a.get("detection").and_then(Value::as_str).unwrap_or("?");
        let Some(class) = Detection::parse(stated_class) else {
            res.errors
                .push(format!("{label}: unknown detection class {stated_class:?}"));
            continue;
        };
        let asset_len = a.get("asset_len").and_then(Value::as_u64);
        if asset_len.is_some_and(|n| Detection::for_len(n as usize, params) != class) {
            res.errors.push(format!(
                "{label}: detection class {stated_class:?} does not match asset_len"
            ));
            continue;
        }
        let mut listed_keys: Vec<Hash> = Vec::new();
        let mut malformed = false;
        for f in listed {
            match f.get("key").and_then(Value::as_str).map(hash_from_hex) {
                Some(Ok(k)) => listed_keys.push(k),
                _ => malformed = true,
            }
        }
        if malformed {
            res.errors
                .push(format!("{label}: malformed fingerprint key"));
            continue;
        }
        if let Some(supplied) = assets {
            let Some(data) = supplied.get(&label) else {
                res.errors
                    .push(format!("{label}: asset bytes not supplied"));
                continue;
            };
            if a.get("asset_sha256").and_then(Value::as_str)
                != Some(prefixed(&sha256(&[data])).as_str())
            {
                res.errors.push(format!(
                    "{label}: supplied asset does not match asset_sha256 in the proof"
                ));
                continue;
            }
            let (recomputed, expected) = asset_fingerprints(data, &salt, params);
            if recomputed != class {
                res.errors.push(format!(
                    "{label}: detection class {stated_class:?} does not match recomputed {:?}",
                    recomputed.as_str()
                ));
                continue;
            }
            let mut sorted = listed_keys.clone();
            sorted.sort();
            if sorted != expected {
                res.errors
                    .push(format!("{label}: fingerprint set does not match the asset"));
                continue;
            }
        }
        let mut present = 0usize;
        for (f, key) in listed.iter().zip(&listed_keys) {
            let short: String = hex::encode(key).chars().take(16).collect();
            let path = match CompactPath::from_json(f.get("path").unwrap_or(&Value::Null)) {
                Ok(p) => p,
                Err(e) => {
                    res.errors
                        .push(format!("{label}: malformed path for {short}: {e}"));
                    continue;
                }
            };
            if f.get("present").and_then(Value::as_bool) == Some(true) {
                if !verify_inclusion(&root, key, &path) {
                    res.errors
                        .push(format!("{label}: inclusion path invalid for {short}"));
                }
                present += 1;
            } else if !verify_non_inclusion(&root, key, &path) {
                res.errors
                    .push(format!("{label}: non-inclusion path invalid for {short}"));
            }
        }
        let v = verdict_for(class, present > 0);
        let stated = a.get("verdict").and_then(Value::as_str).unwrap_or("?");
        if v != stated {
            res.errors.push(format!(
                "{label}: stated verdict {stated:?}, computed {v:?}"
            ));
        }
        if v == VERDICT_UNDETECTABLE {
            res.warnings.push(format!(
                "{label}: shorter than {} bytes, cannot be fingerprinted; not counted as clean",
                params.k_gram
            ));
        }
        if v == VERDICT_ABSENT_PARTIAL {
            res.warnings.push(format!(
                "{label}: shorter than the {}-byte guarantee; exact k-grams absent only",
                params.threshold()
            ));
        }
        res.assets.push((label, v.to_string()));
    }

    let verdicts: Vec<&str> = res.assets.iter().map(|(_, v)| v.as_str()).collect();
    let overall = overall_verdict(&verdicts);
    let stated = proof.get("verdict").and_then(Value::as_str).unwrap_or("?");
    if overall != stated {
        res.errors.push(format!(
            "overall verdict {stated:?} does not match computed {overall:?}"
        ));
    }
    res.verdict = overall.to_string();
    res.ok = res.errors.is_empty();
    res
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Small deterministic PRNG (xorshift64*) so vectors never depend on
    /// the platform's randomness.
    struct Rng(u64);
    impl Rng {
        fn bytes(&mut self, n: usize) -> Vec<u8> {
            (0..n)
                .map(|_| {
                    self.0 ^= self.0 >> 12;
                    self.0 ^= self.0 << 25;
                    self.0 ^= self.0 >> 27;
                    (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8
                })
                .collect()
        }
    }

    const SALT: [u8; 16] = [1u8; 16];

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn witness_with(bodies: &[&[u8]]) -> Witness {
        let mut w = Witness::new("run-test", SALT);
        for (i, b) in bodies.iter().enumerate() {
            w.ingest(b, &format!("2026-09-19T22:00:{i:02}Z"));
        }
        w
    }

    fn assets(items: &[(&str, &[u8])]) -> BTreeMap<String, Vec<u8>> {
        items
            .iter()
            .map(|(l, b)| (l.to_string(), b.to_vec()))
            .collect()
    }

    fn labelled(items: &[(&str, &[u8])]) -> Vec<(String, Vec<u8>)> {
        items
            .iter()
            .map(|(l, b)| (l.to_string(), b.to_vec()))
            .collect()
    }

    #[test]
    fn winnowing_guarantee_holds_at_every_offset() {
        let mut rng = Rng(7);
        let secret = rng.bytes(THRESHOLD);
        let noise = rng.bytes(400);
        let (class, asset_fps) = asset_fingerprints(&secret, &SALT, Params::DEFAULT);
        assert_eq!(class, Detection::Guaranteed);
        for off in 0..=noise.len() {
            let mut body = noise[..off].to_vec();
            body.extend_from_slice(&secret);
            body.extend_from_slice(&noise[off..]);
            let body_fps = fingerprints(&body, &SALT, Params::DEFAULT);
            assert!(
                asset_fps.iter().any(|fp| body_fps.contains(fp)),
                "a shared substring of {THRESHOLD} bytes must hit at offset {off}"
            );
        }
    }

    #[test]
    fn one_byte_short_of_the_threshold_is_only_partial() {
        let mut rng = Rng(11);
        let mid = rng.bytes(THRESHOLD - 1);
        assert_eq!(
            asset_fingerprints(&mid, &SALT, Params::DEFAULT).0,
            Detection::Partial
        );
        assert_eq!(
            asset_fingerprints(&rng.bytes(K_GRAM - 1), &SALT, Params::DEFAULT),
            (Detection::Undetectable, vec![])
        );
        // A partial asset lists every k-gram hash, not a winnowed subset.
        let (_, fps) = asset_fingerprints(&mid, &SALT, Params::DEFAULT);
        assert_eq!(fps.len(), THRESHOLD - 1 - K_GRAM + 1);
    }

    #[test]
    fn absent_and_present_roundtrip() {
        let mut rng = Rng(1);
        let mut leaked = b"sk-live-".to_vec();
        leaked.extend(rng.bytes(60));
        let mut safe = b"AKIA".to_vec();
        safe.extend(rng.bytes(60));
        let mut first = b"POST /v1/chat ".to_vec();
        first.extend(rng.bytes(300));
        first.extend_from_slice(&leaked);
        first.extend_from_slice(b" tail");
        let third = rng.bytes(1000);
        let mut w = witness_with(&[&first, b"GET /health", &third]);
        let sheet = w
            .sheet(&key(), "urn:test:witness", "2026-09-19T22:01:00Z")
            .unwrap();
        assert!(verify_sheet(&sheet).is_empty());
        assert_eq!(sheet["egress"]["bodies"], 3);
        assert_eq!(sheet["egress"]["bytes"], (first.len() + 11 + 1000) as u64);
        assert_eq!(sheet["normalization"], json!(["json-strings-v1"]));

        let proof = w
            .prove(
                &sheet,
                &labelled(&[("openai_key", &leaked), ("aws_key", &safe)]),
            )
            .unwrap();
        assert_eq!(proof["verdict"], VERDICT_PRESENT);
        assert_eq!(proof["assets"][0]["verdict"], VERDICT_PRESENT);
        assert_eq!(proof["assets"][1]["verdict"], VERDICT_ABSENT);
        let res = verify_proof(
            &proof,
            Some(&assets(&[("openai_key", &leaked), ("aws_key", &safe)])),
        );
        assert!(res.ok, "{:?}", res.errors);
        assert_eq!(res.verdict, VERDICT_PRESENT);
        assert!(res.warnings.is_empty());

        let clean = w.prove(&sheet, &labelled(&[("aws_key", &safe)])).unwrap();
        let res = verify_proof(&clean, Some(&assets(&[("aws_key", &safe)])));
        assert!(res.ok && res.verdict == VERDICT_ABSENT);

        // The proof survives a JSON round trip.
        let again: Value = serde_json::from_str(&serde_json::to_string(&clean).unwrap()).unwrap();
        assert!(verify_proof(&again, Some(&assets(&[("aws_key", &safe)]))).ok);
    }

    #[test]
    fn short_assets_are_reported_not_hidden() {
        let mut w = witness_with(&[&Rng(3).bytes(500)]);
        let sheet = w.sheet(&key(), "w", "2026-09-19T22:01:00Z").unwrap();
        let mid = [b'x'; 40];
        let long = [b'y'; 100];
        let proof = w
            .prove(
                &sheet,
                &labelled(&[("pin", b"1234"), ("mid", &mid), ("long", &long)]),
            )
            .unwrap();
        let by: BTreeMap<&str, &str> = proof["assets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| (a["label"].as_str().unwrap(), a["verdict"].as_str().unwrap()))
            .collect();
        assert_eq!(by["pin"], VERDICT_UNDETECTABLE);
        assert_eq!(by["mid"], VERDICT_ABSENT_PARTIAL);
        assert_eq!(by["long"], VERDICT_ABSENT);
        assert_eq!(proof["verdict"], VERDICT_MIXED);
        let res = verify_proof(
            &proof,
            Some(&assets(&[("pin", b"1234"), ("mid", &mid), ("long", &long)])),
        );
        assert!(res.ok, "{:?}", res.errors);
        assert!(
            res.warnings
                .iter()
                .any(|m| m.contains("cannot be fingerprinted"))
        );
        assert!(res.warnings.iter().any(|m| m.contains("guarantee")));
    }

    #[test]
    fn json_strings_restore_the_guarantee_for_quoted_files() {
        // Every line is shorter than a k-gram, so raw body and file share
        // no k-gram at all: only the decoded string can match.
        let file =
            "TOKEN=abcdef0123456789\nDB=postgres://h/db\nREGION=eu-west-1\nMODE=production\n";
        let body = serde_json::to_vec(&json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": format!("here is my .env:\n{file}")}]
        }))
        .unwrap();
        // Raw bytes carry `\n` as two characters, so the file's own bytes
        // never appear contiguously in the body.
        let mut raw_only = Witness::new("raw", SALT);
        raw_only.normalization.clear();
        raw_only.ingest(&body, "2026-09-19T22:00:00Z");
        let (_, fps) = asset_fingerprints(file.as_bytes(), &SALT, Params::DEFAULT);
        assert!(!fps.iter().any(|fp| raw_only.map.contains(fp)));

        let mut normalised = witness_with(&[&body]);
        assert!(fps.iter().any(|fp| normalised.map.contains(fp)));
        let sheet = normalised
            .sheet(&key(), "w", "2026-09-19T22:01:00Z")
            .unwrap();
        let proof = normalised
            .prove(&sheet, &labelled(&[(".env", file.as_bytes())]))
            .unwrap();
        assert_eq!(proof["verdict"], VERDICT_PRESENT);

        // Derived bodies never count as egress.
        assert_eq!(normalised.bodies, 1);
        assert_eq!(normalised.bytes, body.len() as u64);
        assert!(json_strings(b"not json", K_GRAM).is_empty());
        assert!(json_strings(&[0xff, 0xfe], K_GRAM).is_empty());
        assert_eq!(json_strings(br#"{"a":"short","b":["x"]}"#, K_GRAM).len(), 0);
    }

    #[test]
    fn tampering_is_detected() {
        let mut rng = Rng(5);
        let secret = rng.bytes(80);
        let mut w = witness_with(&[&rng.bytes(400)]);
        let sheet = w.sheet(&key(), "w", "2026-09-19T22:01:00Z").unwrap();
        let proof = w.prove(&sheet, &labelled(&[("s", &secret)])).unwrap();
        assert!(verify_proof(&proof, Some(&assets(&[("s", &secret)]))).ok);

        // Wrong asset bytes.
        let other = rng.bytes(80);
        assert!(!verify_proof(&proof, Some(&assets(&[("s", &other)]))).ok);

        // Forged verdict.
        let mut bad = proof.clone();
        bad["assets"][0]["verdict"] = json!(VERDICT_PRESENT);
        let r = verify_proof(&bad, Some(&assets(&[("s", &secret)])));
        assert!(!r.ok && r.errors.iter().any(|e| e.contains("stated verdict")));

        // Tampered sheet (egress count changed after signing).
        let mut bad = proof.clone();
        bad["sheet"]["egress"]["bodies"] = json!(0);
        let r = verify_proof(&bad, Some(&assets(&[("s", &secret)])));
        assert!(!r.ok && r.errors.iter().any(|e| e.contains("signature")));

        // A path against another root.
        let mut other_w = witness_with(&[&rng.bytes(400)]);
        let other_sheet = other_w.sheet(&key(), "w", "2026-09-19T22:01:00Z").unwrap();
        let mut bad = proof.clone();
        bad["sheet"] = other_sheet;
        let r = verify_proof(&bad, Some(&assets(&[("s", &secret)])));
        assert!(!r.ok && r.errors.iter().any(|e| e.contains("path invalid")));

        // Unknown normalisation layer is refused even when signed.
        let mut w2 = witness_with(&[b"body body body body body body body body body"]);
        w2.normalization = vec!["base64-v9".into()];
        let s2 = w2.sheet(&key(), "w", "2026-09-19T22:01:00Z").unwrap();
        assert!(
            verify_sheet(&s2)
                .iter()
                .any(|e| e.contains("normalization"))
        );

        // Prover cannot use a sheet from another map.
        assert!(w.prove(&s2, &labelled(&[("s", &secret)])).is_err());
    }

    #[test]
    fn hash_only_mode_warns() {
        let mut rng = Rng(9);
        let secret = rng.bytes(80);
        let mut w = witness_with(&[&rng.bytes(400)]);
        let sheet = w.sheet(&key(), "w", "2026-09-19T22:01:00Z").unwrap();
        let proof = w.prove(&sheet, &labelled(&[("s", &secret)])).unwrap();
        let res = verify_proof(&proof, None);
        assert!(res.ok && res.verdict == VERDICT_ABSENT);
        assert!(res.warnings.iter().any(|m| m.contains("not recomputed")));
        // Supplying some but not all assets is not hash-only mode: refused.
        let two = w
            .prove(&sheet, &labelled(&[("s", &secret), ("t", &rng.bytes(64))]))
            .unwrap();
        let r = verify_proof(&two, Some(&assets(&[("s", &secret)])));
        assert!(!r.ok && r.errors.iter().any(|e| e.contains("not supplied")));
    }

    #[test]
    fn sparse_merkle_map_paths_prove_membership_and_absence() {
        let mut m = SparseMerkleMap::new();
        assert_eq!(m.root(), EMPTY[DEPTH]);
        let mut rng = Rng(13);
        let keys: Vec<Hash> = (0..20).map(|_| rng.bytes(32).try_into().unwrap()).collect();
        for k in &keys {
            assert!(m.insert(*k));
            assert!(!m.insert(*k));
        }
        assert_eq!(m.len(), 20);
        let root = m.root();
        for k in &keys {
            let path = m.prove(k);
            assert!(verify_inclusion(&root, k, &path));
            assert!(!verify_non_inclusion(&root, k, &path));
            let rt = CompactPath::from_json(&path.to_json()).unwrap();
            assert_eq!(rt, path);
        }
        let absent: Hash = rng.bytes(32).try_into().unwrap();
        let path = m.prove(&absent);
        assert!(verify_non_inclusion(&root, &absent, &path));
        assert!(!verify_inclusion(&root, &absent, &path));
        // Against a foreign root the same path fails.
        m.insert(rng.bytes(32).try_into().unwrap());
        let other_root = m.root();
        assert_ne!(root, other_root);
        assert!(!verify_non_inclusion(&other_root, &absent, &path));
        // Malformed paths verify as false, never panic.
        let mut broken = path.clone();
        broken.siblings.pop();
        assert!(!verify_non_inclusion(&root, &absent, &broken));
        assert!(CompactPath::from_json(&json!({"bitmap": "zz", "siblings": []})).is_err());
    }

    #[test]
    fn single_key_chain_matches_the_recursive_definition() {
        // The memoised shortcut for one-key subtrees must hash exactly what
        // the reference's recursion hashes.
        let key: Hash = Rng(21).bytes(32).try_into().unwrap();
        let mut m = SparseMerkleMap::new();
        m.insert(key);
        let root = m.root();
        let mut node = leaf_hash(&key, &PRESENT);
        for depth in (0..DEPTH).rev() {
            let h = DEPTH - 1 - depth;
            node = if key_bit(&key, depth) == 1 {
                node_hash(&EMPTY[h], &node)
            } else {
                node_hash(&node, &EMPTY[h])
            };
        }
        assert_eq!(root, node);
    }

    #[test]
    fn sheet_signature_domain_has_a_trailing_newline_like_the_reference() {
        let payload = sheet_payload(&json!({"a": 1, "signature": {"x": "y"}})).unwrap();
        assert!(payload.starts_with(b"CROVIA-PNX-SHEET-v1\n{\"a\":1}"));
        assert_eq!(payload.len(), DOMAIN_SHEET.len() + 7);
    }

    // -- Vectors generated by the Python reference (tests/vectors/pnx/,
    // `generate.py`). These run everywhere, Python or not.

    const VEC_FINGERPRINTS: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/vectors/pnx/fingerprints.json"
    ));
    const VEC_SMT: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/vectors/pnx/smt.json"
    ));
    const VEC_WITNESS: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/vectors/pnx/witness.json"
    ));

    fn vec_bytes(v: &Value) -> Vec<u8> {
        if let Some(h) = v.get("hex").and_then(Value::as_str) {
            hex::decode(h).unwrap()
        } else {
            v.get("text")
                .and_then(Value::as_str)
                .unwrap()
                .as_bytes()
                .to_vec()
        }
    }

    fn hex_set(v: &Value) -> Vec<Hash> {
        let mut out: Vec<Hash> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|s| hash_from_hex(s.as_str().unwrap()).unwrap())
            .collect();
        out.sort();
        out
    }

    #[test]
    fn vectors_fingerprints_match_the_reference() {
        let doc: Value = serde_json::from_str(VEC_FINGERPRINTS).unwrap();
        let salt: [u8; 16] = hex::decode(doc["salt_hex"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
        let p = Params {
            k_gram: doc["k_gram"].as_u64().unwrap() as usize,
            window: doc["window"].as_u64().unwrap() as usize,
        };
        for case in doc["bodies"].as_array().unwrap() {
            let name = case["name"].as_str().unwrap();
            let body = vec_bytes(&case["body"]);
            let raw: Vec<Hash> = fingerprints(&body, &salt, p).into_iter().collect();
            assert_eq!(
                raw,
                hex_set(&case["raw_fingerprints"]),
                "raw fingerprints of {name}"
            );
            // Document order differs (serde_json sorts object keys); the
            // fingerprint set does not depend on it.
            let mut derived: Vec<Vec<u8>> = case["json_strings"]
                .as_array()
                .unwrap()
                .iter()
                .map(vec_bytes)
                .collect();
            derived.sort();
            let mut ours = json_strings(&body, p.k_gram);
            ours.sort();
            assert_eq!(ours, derived, "json-strings-v1 of {name}");
            let kg = kgram_hashes(&body, &salt, p.k_gram);
            assert_eq!(
                kg.len() as u64,
                case["kgram_count"].as_u64().unwrap(),
                "k-gram count of {name}"
            );
            if let Some(first) = case.get("first_kgram_hash").and_then(Value::as_str) {
                assert_eq!(hex::encode(kg[0]), first, "first k-gram hash of {name}");
            }
        }
        for case in doc["assets"].as_array().unwrap() {
            let name = case["name"].as_str().unwrap();
            let (class, fps) = asset_fingerprints(&vec_bytes(&case["asset"]), &salt, p);
            assert_eq!(
                class.as_str(),
                case["detection"].as_str().unwrap(),
                "class of {name}"
            );
            assert_eq!(
                fps,
                hex_set(&case["fingerprints"]),
                "fingerprints of {name}"
            );
        }
    }

    #[test]
    fn vectors_sparse_merkle_map_matches_the_reference() {
        let doc: Value = serde_json::from_str(VEC_SMT).unwrap();
        assert_eq!(
            hex::encode(EMPTY[DEPTH]),
            doc["empty_root"].as_str().unwrap()
        );
        assert_eq!(
            hex::encode(*PRESENT),
            doc["present_leaf_value"].as_str().unwrap()
        );
        for case in doc["cases"].as_array().unwrap() {
            let name = case["name"].as_str().unwrap();
            let mut m = SparseMerkleMap::new();
            for k in case["keys"].as_array().unwrap() {
                m.insert(hash_from_hex(k.as_str().unwrap()).unwrap());
            }
            let root = m.root();
            assert_eq!(
                hex::encode(root),
                case["root"].as_str().unwrap(),
                "root of {name}"
            );
            for p in case["paths"].as_array().unwrap() {
                let key = hash_from_hex(p["key"].as_str().unwrap()).unwrap();
                let path = m.prove(&key);
                assert_eq!(
                    path.to_json(),
                    p["path"],
                    "path encoding for {name}/{}",
                    p["key"]
                );
                let present = p["present"].as_bool().unwrap();
                assert_eq!(m.contains(&key), present);
                if present {
                    assert!(verify_inclusion(&root, &key, &path));
                } else {
                    assert!(verify_non_inclusion(&root, &key, &path));
                }
            }
        }
    }

    #[test]
    fn vectors_witness_sheet_and_proofs_match_the_reference() {
        let doc: Value = serde_json::from_str(VEC_WITNESS).unwrap();
        let salt: [u8; 16] = hex::decode(doc["salt_hex"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
        let mut w = Witness::new(doc["run_id"].as_str().unwrap(), salt);
        for b in doc["bodies"].as_array().unwrap() {
            w.ingest(&vec_bytes(&b["body"]), b["at"].as_str().unwrap());
        }
        assert_eq!(prefixed(&w.root()), doc["root"].as_str().unwrap());
        assert_eq!(w.map.len() as u64, doc["fingerprints"].as_u64().unwrap());

        // Same key, same close time: the Rust sheet is byte-for-byte the
        // Python sheet (Ed25519 is deterministic).
        let seed: [u8; 32] = hex::decode(doc["witness_seed_hex"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
        let k = SigningKey::from_bytes(&seed);
        let ref_sheet = &doc["sheet"];
        let sheet = w
            .sheet(
                &k,
                ref_sheet["witness"]["id"].as_str().unwrap(),
                ref_sheet["closed_at"].as_str().unwrap(),
            )
            .unwrap();
        assert_eq!(
            csc1_serialize(&sheet).unwrap(),
            csc1_serialize(ref_sheet).unwrap(),
            "sheet differs from the reference"
        );
        assert!(verify_sheet(ref_sheet).is_empty());

        for case in doc["proofs"].as_array().unwrap() {
            let name = case["name"].as_str().unwrap();
            let supplied: BTreeMap<String, Vec<u8>> = case["assets"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(l, b)| (l.clone(), vec_bytes(b)))
                .collect();
            // The Python proof verifies here, with and without asset bytes.
            let res = verify_proof(&case["proof"], Some(&supplied));
            assert!(res.ok, "{name}: {:?}", res.errors);
            assert_eq!(res.verdict, case["verdict"].as_str().unwrap(), "{name}");
            let expected: BTreeMap<String, String> = case["asset_verdicts"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(l, v)| (l.clone(), v.as_str().unwrap().to_string()))
                .collect();
            let got: BTreeMap<String, String> = res.assets.iter().cloned().collect();
            assert_eq!(got, expected, "{name}");
            let hash_only = verify_proof(&case["proof"], None);
            assert!(
                hash_only.ok
                    && hash_only
                        .warnings
                        .iter()
                        .any(|m| m.contains("not recomputed"))
            );

            // The Rust proof for the same assets is the same object.
            let ordered: Vec<(String, Vec<u8>)> = case["proof"]["assets"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| {
                    let l = a["label"].as_str().unwrap().to_string();
                    let b = supplied[&l].clone();
                    (l, b)
                })
                .collect();
            let ours = w.prove(&sheet, &ordered).unwrap();
            assert_eq!(
                csc1_serialize(&ours).unwrap(),
                csc1_serialize(&case["proof"]).unwrap(),
                "{name}: proof differs from the reference"
            );
        }
    }

    // -- Live cross-check against the Python reference when it is
    // importable (`python3 -c "import tacet.egress"`); skipped otherwise.

    fn python_reference() -> Option<std::process::Command> {
        let mut cmd = std::process::Command::new("python3");
        if let Ok(dir) = std::env::var("TACET_REFERENCE_PYTHON") {
            cmd.env("PYTHONPATH", dir);
        }
        let probe = cmd
            .args(["-c", "import tacet.egress, tacet.smt, tacet.keys"])
            .output()
            .ok()?;
        if !probe.status.success() {
            eprintln!(
                "skipping live cross-check: the Python reference (crovia-tacet) is not importable; \
                 set TACET_REFERENCE_PYTHON to reference/python to enable it"
            );
            return None;
        }
        let mut fresh = std::process::Command::new("python3");
        if let Ok(dir) = std::env::var("TACET_REFERENCE_PYTHON") {
            fresh.env("PYTHONPATH", dir);
        }
        Some(fresh)
    }

    fn run_python(mut cmd: std::process::Command, script: &str, stdin: &[u8]) -> Value {
        use std::io::Write;
        let mut child = cmd
            .args(["-c", script])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .expect("python3 spawns");
        child.stdin.take().unwrap().write_all(stdin).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "python reference failed");
        serde_json::from_slice(&out.stdout).expect("python emits JSON")
    }

    #[test]
    fn live_python_reference_agrees_on_root_sheet_and_proofs() {
        let Some(cmd) = python_reference() else {
            return;
        };
        let mut rng = Rng(77);
        let secret = rng.bytes(80);
        let mut file = String::from("API_KEY=");
        file.push_str(&hex::encode(rng.bytes(24)));
        file.push_str("\nOTHER=value\n");
        let bodies: Vec<Vec<u8>> = vec![
            {
                let mut b = rng.bytes(300);
                b.extend_from_slice(&secret);
                b.extend(rng.bytes(50));
                b
            },
            serde_json::to_vec(&json!({"model": "gpt-4o", "messages": [
                {"role": "system", "content": "You are a careful assistant that reviews code."},
                {"role": "user", "content": format!("config:\n{file}")}]}))
            .unwrap(),
            b"GET /health".to_vec(),
            rng.bytes(700),
        ];
        let safe = rng.bytes(64);
        let mid = rng.bytes(40);
        let input = json!({
            "salt_hex": hex::encode(SALT),
            "seed_hex": hex::encode([7u8; 32]),
            "witness_id": "urn:test:witness",
            "closed_at": "2026-09-20T12:00:00Z",
            "bodies": bodies.iter().map(hex::encode).collect::<Vec<_>>(),
            "assets": [["secret", hex::encode(&secret)], ["env", hex::encode(file.as_bytes())],
                       ["safe", hex::encode(&safe)], ["mid", hex::encode(&mid)], ["pin", hex::encode(b"1234")]],
        });
        let script = r#"
import json, sys
from tacet import egress
from tacet.keys import SigningKey
inp = json.load(sys.stdin)
w = egress.EgressWitness(run_id="run-live", salt=bytes.fromhex(inp["salt_hex"]))
for i, b in enumerate(inp["bodies"]):
    w.ingest(bytes.fromhex(b), f"2026-09-20T11:00:{i:02d}Z")
key = SigningKey.from_seed(inp["witness_id"], bytes.fromhex(inp["seed_hex"]))
sheet = w.sheet(key, inp["closed_at"])
assets = [(l, bytes.fromhex(h)) for l, h in inp["assets"]]
proof = w.prove(sheet, assets)
json.dump({"root": sheet["root"], "fingerprints": len(w._map), "sheet": sheet, "proof": proof}, sys.stdout)
"#;
        let py = run_python(
            cmd,
            script,
            serde_json::to_string(&input).unwrap().as_bytes(),
        );

        let mut w = Witness::new("run-live", SALT);
        for (i, b) in bodies.iter().enumerate() {
            w.ingest(b, &format!("2026-09-20T11:00:{i:02}Z"));
        }
        assert_eq!(
            prefixed(&w.root()),
            py["root"].as_str().unwrap(),
            "roots differ"
        );
        assert_eq!(w.map.len() as u64, py["fingerprints"].as_u64().unwrap());
        let sheet = w
            .sheet(&key(), "urn:test:witness", "2026-09-20T12:00:00Z")
            .unwrap();
        assert_eq!(
            csc1_serialize(&sheet).unwrap(),
            csc1_serialize(&py["sheet"]).unwrap()
        );
        let labelled_assets = labelled(&[
            ("secret", &secret),
            ("env", file.as_bytes()),
            ("safe", &safe),
            ("mid", &mid),
            ("pin", b"1234"),
        ]);
        let ours = w.prove(&sheet, &labelled_assets).unwrap();
        assert_eq!(
            csc1_serialize(&ours).unwrap(),
            csc1_serialize(&py["proof"]).unwrap(),
            "proofs differ"
        );
        let supplied: BTreeMap<String, Vec<u8>> = labelled_assets.into_iter().collect();
        let res = verify_proof(&py["proof"], Some(&supplied));
        assert!(res.ok, "{:?}", res.errors);
        assert_eq!(res.verdict, VERDICT_PRESENT);
        let got: BTreeMap<String, String> = res.assets.into_iter().collect();
        assert_eq!(got["secret"], VERDICT_PRESENT);
        assert_eq!(got["env"], VERDICT_PRESENT);
        assert_eq!(got["safe"], VERDICT_ABSENT);
        assert_eq!(got["mid"], VERDICT_ABSENT_PARTIAL);
        assert_eq!(got["pin"], VERDICT_UNDETECTABLE);
    }

    #[test]
    fn live_python_reference_verifies_rust_proofs_and_rejects_tampering() {
        let Some(cmd) = python_reference() else {
            return;
        };
        let mut rng = Rng(78);
        let secret = rng.bytes(64);
        let safe = rng.bytes(64);
        let mut body = rng.bytes(200);
        body.extend_from_slice(&secret);
        let mut w = witness_with(&[&body, &rng.bytes(500)]);
        let sheet = w
            .sheet(&key(), "urn:test:witness", "2026-09-20T12:00:00Z")
            .unwrap();
        let clean = w.prove(&sheet, &labelled(&[("safe", &safe)])).unwrap();
        let exposed = w
            .prove(&sheet, &labelled(&[("secret", &secret), ("safe", &safe)]))
            .unwrap();
        let mut forged = clean.clone();
        forged["sheet"]["egress"]["bytes"] = json!(1);
        let input = json!({
            "assets": {"safe": hex::encode(&safe), "secret": hex::encode(&secret)},
            "proofs": {"clean": clean, "exposed": exposed, "forged": forged},
        });
        let script = r#"
import json, sys
from tacet import egress
inp = json.load(sys.stdin)
assets = {l: bytes.fromhex(h) for l, h in inp["assets"].items()}
out = {}
for name, proof in inp["proofs"].items():
    r = egress.verify_pnx(proof, {l: assets[l] for l in [a["label"] for a in proof["assets"]]})
    h = egress.verify_pnx(proof)
    out[name] = {"ok": r.ok, "verdict": r.verdict, "errors": r.errors, "hash_only_ok": h.ok, "hash_only_warnings": h.warnings}
json.dump(out, sys.stdout)
"#;
        let py = run_python(
            cmd,
            script,
            serde_json::to_string(&input).unwrap().as_bytes(),
        );
        assert_eq!(py["clean"]["ok"], true, "{}", py["clean"]);
        assert_eq!(py["clean"]["verdict"], VERDICT_ABSENT);
        assert_eq!(py["clean"]["hash_only_ok"], true);
        assert!(
            py["clean"]["hash_only_warnings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m.as_str().unwrap().contains("not recomputed"))
        );
        assert_eq!(py["exposed"]["ok"], true, "{}", py["exposed"]);
        assert_eq!(py["exposed"]["verdict"], VERDICT_PRESENT);
        assert_eq!(py["forged"]["ok"], false);
        assert!(py["forged"]["errors"].to_string().contains("signature"));
    }
}
