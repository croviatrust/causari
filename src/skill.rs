use anyhow::{Context, Result, anyhow};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::dag;
use crate::object::{canonical_json, hash_bytes};
use crate::repo::Repo;
use crate::snapshot::flatten_tree;
use crate::store::Store;

// LAYER 3 — EXPERIENCE.
//
// The ledger (layers 1-2) records what agents did. This module turns that
// record into something agents can *reuse*: a skill. A skill is a distilled,
// Ed25519-signed unit of proven experience — the prompt that triggered the
// work, the steps that were taken, and the evidence that it worked
// (exit code 0, or the code surviving at the tip of the timeline).
//
// Trust is earned, never claimed:
//
//   ● recorded  — distilled from the ledger, no success signal yet
//   ◆ verified  — at least one success signal (exit 0, or work survived)
//   ★ proven    — verified AND recalled 3+ times by agents doing new work
//
// The signature makes skills portable: any Causari binary can verify that a
// skill file was produced by the keypair of this repository and was not
// edited after signing. Tampered skills are flagged, not trusted.

pub const SKILL_SCHEMA: &str = "causari.skill.v0.1";
const PROVEN_USES: u64 = 3;

/// One step of a skill: a single event, reduced to what matters for reuse.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillStep {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub writes: Vec<String>,
}

/// Success evidence gathered at distillation time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Verification {
    /// Some source event finished with exit code 0.
    pub exit_zero: bool,
    /// Every file the skill touched still exists at the timeline tip —
    /// the work was not reverted or deleted.
    pub survived: bool,
}

/// The immutable, signed core of a skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillCore {
    pub schema: String,
    /// Short human title (first line of the trigger prompt).
    pub title: String,
    /// The prompt that drove the work. This is what recall matches against.
    pub trigger: String,
    pub steps: Vec<SkillStep>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Ledger events this skill was distilled from (full ids).
    pub source_events: Vec<String>,
    /// Union of files the steps wrote.
    pub files: Vec<String>,
    pub verification: Verification,
    pub created_at: String,
}

/// Mutable usage counters, outside the signature.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillStats {
    pub uses: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<String>,
}

/// Provenance metadata for imported / mesh-synced skills. NOT signed — the
/// Ed25519 signature covers only `skill`; this records where it came from.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillMeshMeta {
    /// "local" or the label of a trusted org key that signed this skill.
    pub signer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imported_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imported_at: Option<String>,
}

/// What is stored on disk: signed core + open stats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillEnvelope {
    pub skill: SkillCore,
    /// hex Ed25519 public key (32 bytes)
    pub public_key: String,
    /// hex Ed25519 signature over canonical_json(skill) (64 bytes)
    pub signature: String,
    #[serde(default)]
    pub stats: SkillStats,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh: Option<SkillMeshMeta>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    Recorded,
    Verified,
    Proven,
}

impl Trust {
    pub fn as_str(&self) -> &'static str {
        match self {
            Trust::Recorded => "recorded",
            Trust::Verified => "verified",
            Trust::Proven => "proven",
        }
    }

    pub fn badge(&self) -> &'static str {
        match self {
            Trust::Recorded => "●",
            Trust::Verified => "◆",
            Trust::Proven => "★",
        }
    }
}

impl SkillEnvelope {
    pub fn trust(&self) -> Trust {
        let verified = self.skill.verification.exit_zero || self.skill.verification.survived;
        if verified && self.stats.uses >= PROVEN_USES {
            Trust::Proven
        } else if verified {
            Trust::Verified
        } else {
            Trust::Recorded
        }
    }
}

// ---------- identity & signature ----------

/// Content-addressed skill id: BLAKE3 of the signed core.
pub fn skill_id(core: &SkillCore) -> Result<String> {
    Ok(hash_bytes(&canonical_json(core)?))
}

pub fn keys_dir(repo: &Repo) -> PathBuf {
    repo.dir.join("keys")
}

fn signing_key_path(repo: &Repo) -> PathBuf {
    keys_dir(repo).join("skill-signing.key")
}

/// Load the repo's skill-signing key, generating one on first use.
pub fn load_or_create_signing_key(repo: &Repo) -> Result<SigningKey> {
    let path = signing_key_path(repo);
    if path.exists() {
        let hex_str = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let bytes = hex::decode(hex_str.trim()).context("decoding signing key")?;
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| anyhow!("signing key must be 32 bytes"))?;
        return Ok(SigningKey::from_bytes(&arr));
    }
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).map_err(|e| anyhow!("generating key: {}", e))?;
    let key = SigningKey::from_bytes(&secret);
    std::fs::create_dir_all(keys_dir(repo))?;
    std::fs::write(&path, hex::encode(secret))?;
    // Public key alongside, for sharing/verification by other parties.
    std::fs::write(
        keys_dir(repo).join("skill-signing.pub"),
        hex::encode(key.verifying_key().to_bytes()),
    )?;
    Ok(key)
}

pub fn sign_skill(core: SkillCore, key: &SigningKey) -> Result<SkillEnvelope> {
    let msg = canonical_json(&core)?;
    let sig = key.sign(&msg);
    Ok(SkillEnvelope {
        skill: core,
        public_key: hex::encode(key.verifying_key().to_bytes()),
        signature: hex::encode(sig.to_bytes()),
        stats: SkillStats::default(),
        mesh: None,
    })
}

/// Check the envelope's signature. Ok(()) means: this exact skill content
/// was signed by the embedded public key and not modified since.
pub fn verify_envelope(env: &SkillEnvelope) -> Result<()> {
    let pk_bytes: [u8; 32] = hex::decode(&env.public_key)
        .context("decoding public key")?
        .try_into()
        .map_err(|_| anyhow!("public key must be 32 bytes"))?;
    let sig_bytes: [u8; 64] = hex::decode(&env.signature)
        .context("decoding signature")?
        .try_into()
        .map_err(|_| anyhow!("signature must be 64 bytes"))?;
    let vk = VerifyingKey::from_bytes(&pk_bytes).context("invalid public key")?;
    let sig = Signature::from_bytes(&sig_bytes);
    let msg = canonical_json(&env.skill)?;
    vk.verify(&msg, &sig)
        .map_err(|_| anyhow!("signature verification FAILED — skill was modified after signing"))
}

// ---------- trust plane (cross-repo skill mesh) ----------

pub const BUNDLE_SCHEMA: &str = "causari.skill.bundle.v0.1";

/// Portable export wrapper — skill + export metadata, still signature-bound
/// to the original signer inside `envelope`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillBundle {
    pub schema: String,
    pub exported_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    pub envelope: SkillEnvelope,
}

pub fn trusted_dir(repo: &Repo) -> PathBuf {
    keys_dir(repo).join("trusted")
}

/// Hex-encoded public key of this repo's skill signer (if any).
pub fn local_public_key_hex(repo: &Repo) -> Result<Option<String>> {
    let path = keys_dir(repo).join("skill-signing.pub");
    if !path.exists() {
        return Ok(None);
    }
    let hex = std::fs::read_to_string(&path)?.trim().to_string();
    if hex.len() == 64 {
        Ok(Some(hex))
    } else {
        Ok(None)
    }
}

fn parse_pubkey_hex(input: &str) -> Result<String> {
    let s = input.trim();
    if s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(s.to_lowercase());
    }
    // Treat as path to a .pub file.
    let raw = std::fs::read_to_string(s).with_context(|| format!("reading key file {}", s))?;
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if line.len() == 64 && line.chars().all(|c| c.is_ascii_hexdigit()) {
            return Ok(line.to_lowercase());
        }
    }
    Err(anyhow!(
        "expected 64-char hex Ed25519 public key or a .pub file containing one"
    ))
}

/// Register an org/team public key by label. Teammates run `re skill trust add`
/// with your pubkey; then `re skill pull` accepts skills you signed.
pub fn trust_add(repo: &Repo, label: &str, key: &str) -> Result<()> {
    if label.contains(['/', '\\', ' ', '\t']) || label.is_empty() {
        return Err(anyhow!("trust label must be a simple identifier"));
    }
    let hex = parse_pubkey_hex(key)?;
    std::fs::create_dir_all(trusted_dir(repo))?;
    std::fs::write(
        trusted_dir(repo).join(format!("{}.pub", label)),
        format!("# {}\n{}\n", label, hex),
    )?;
    Ok(())
}

pub fn trust_remove(repo: &Repo, label: &str) -> Result<()> {
    let path = trusted_dir(repo).join(format!("{}.pub", label));
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

/// All trusted org keys: (label, pubkey_hex).
pub fn list_trusted_keys(repo: &Repo) -> Result<Vec<(String, String)>> {
    let dir = trusted_dir(repo);
    let mut out = Vec::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(label) = name.strip_suffix(".pub") else {
            continue;
        };
        let hex = parse_pubkey_hex(entry.path().to_str().unwrap_or(""))?;
        out.push((label.to_string(), hex));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Resolve which trusted label (if any) matches this public key.
pub fn trusted_label_for(repo: &Repo, pubkey_hex: &str) -> Result<Option<String>> {
    let pk = pubkey_hex.to_lowercase();
    for (label, hex) in list_trusted_keys(repo)? {
        if hex == pk {
            return Ok(Some(label));
        }
    }
    Ok(None)
}

/// Whether this repo will accept skills signed by `pubkey_hex`.
pub fn is_acceptable_signer(repo: &Repo, pubkey_hex: &str) -> Result<bool> {
    verify_envelope_pubkey_only(pubkey_hex)?; // sanity: valid hex length
    let pk = pubkey_hex.to_lowercase();
    if local_public_key_hex(repo)?.as_deref() == Some(pk.as_str()) {
        return Ok(true);
    }
    Ok(trusted_label_for(repo, &pk)?.is_some())
}

fn verify_envelope_pubkey_only(pubkey_hex: &str) -> Result<()> {
    let pk_bytes: [u8; 32] = hex::decode(pubkey_hex.trim())
        .context("decoding public key")?
        .try_into()
        .map_err(|_| anyhow!("public key must be 32 bytes"))?;
    VerifyingKey::from_bytes(&pk_bytes).context("invalid public key")?;
    Ok(())
}

fn repo_origin_hint(repo: &Repo) -> String {
    repo.root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("repo")
        .to_string()
}

pub fn export_bundle(repo: &Repo, id_prefix: &str) -> Result<SkillBundle> {
    let (_, env) = find_skill(repo, id_prefix)?;
    verify_envelope(&env)?;
    Ok(SkillBundle {
        schema: BUNDLE_SCHEMA.to_string(),
        exported_at: chrono::Utc::now().to_rfc3339(),
        origin: Some(repo_origin_hint(repo)),
        envelope: env,
    })
}

pub fn write_bundle(path: &std::path::Path, bundle: &SkillBundle) -> Result<()> {
    let json = serde_json::to_string_pretty(bundle)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, json)?;
    Ok(())
}

/// Parse a bundle file or a bare SkillEnvelope JSON.
pub fn read_bundle_file(path: &std::path::Path) -> Result<SkillEnvelope> {
    let raw = std::fs::read_to_string(path)?;
    if let Ok(bundle) = serde_json::from_str::<SkillBundle>(&raw) {
        return Ok(bundle.envelope);
    }
    serde_json::from_str(&raw).context("parsing skill JSON (expected bundle or envelope)")
}

pub struct ImportReport {
    pub imported: Vec<(String, SkillEnvelope)>,
    pub skipped_existing: usize,
    pub rejected: Vec<(String, String)>,
}

/// Import a signed skill if the signer is local or trusted.
pub fn import_envelope(
    repo: &Repo,
    mut env: SkillEnvelope,
    source: Option<&str>,
) -> Result<(String, bool)> {
    verify_envelope(&env)?;
    if !is_acceptable_signer(repo, &env.public_key)? {
        return Err(anyhow!(
            "signer {} is not local and not in trusted keys — run `re skill trust add` first",
            &env.public_key[..16]
        ));
    }
    let id = skill_id(&env.skill)?;
    if skill_path(repo, &id).exists() {
        return Ok((id, false));
    }
    let signer = if local_public_key_hex(repo)?.as_deref() == Some(env.public_key.as_str()) {
        "local".to_string()
    } else {
        trusted_label_for(repo, &env.public_key)?.unwrap_or_else(|| "trusted".to_string())
    };
    env.mesh = Some(SkillMeshMeta {
        signer,
        imported_from: source.map(String::from),
        imported_at: Some(chrono::Utc::now().to_rfc3339()),
    });
    save_skill(repo, &id, &env)?;
    Ok((id, true))
}

pub fn import_file(repo: &Repo, path: &std::path::Path) -> Result<(String, bool)> {
    let env = read_bundle_file(path)?;
    import_envelope(repo, env, Some(&path.to_string_lossy().replace('\\', "/")))
}

/// Sync every `.json` skill bundle from a team directory (Dropbox, git repo,
/// NFS — anything that looks like a folder). No server. No accounts.
pub fn pull_from_dir(repo: &Repo, dir: &std::path::Path) -> Result<ImportReport> {
    let mut report = ImportReport {
        imported: Vec::new(),
        skipped_existing: 0,
        rejected: Vec::new(),
    };
    if !dir.is_dir() {
        return Err(anyhow!("{} is not a directory", dir.display()));
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        match read_bundle_file(&path) {
            Ok(env) => {
                match import_envelope(repo, env, Some(&path.to_string_lossy().replace('\\', "/"))) {
                    Ok((id, true)) => {
                        let (_, saved) = find_skill(repo, &id)?;
                        report.imported.push((id, saved));
                    }
                    Ok((_, false)) => report.skipped_existing += 1,
                    Err(e) => report.rejected.push((name, e.to_string())),
                }
            }
            Err(e) => report.rejected.push((name, e.to_string())),
        }
    }
    Ok(report)
}

// ---------- storage ----------

pub fn skills_dir(repo: &Repo) -> PathBuf {
    repo.dir.join("skills")
}

fn skill_path(repo: &Repo, id: &str) -> PathBuf {
    skills_dir(repo).join(format!("{}.json", &id[..32.min(id.len())]))
}

pub fn save_skill(repo: &Repo, id: &str, env: &SkillEnvelope) -> Result<()> {
    std::fs::create_dir_all(skills_dir(repo))?;
    let json = serde_json::to_string_pretty(env)?;
    std::fs::write(skill_path(repo, id), json)?;
    Ok(())
}

pub fn load_skills(repo: &Repo) -> Result<Vec<(String, SkillEnvelope)>> {
    let dir = skills_dir(repo);
    let mut out = Vec::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let raw = std::fs::read_to_string(&path)?;
        let env: SkillEnvelope = match serde_json::from_str(&raw) {
            Ok(e) => e,
            Err(_) => continue, // unreadable file: surfaced by `re skill verify`
        };
        let id = skill_id(&env.skill)?;
        out.push((id, env));
    }
    // Newest first, then by title for stability.
    out.sort_by(|a, b| {
        (&b.1.skill.created_at, &a.1.skill.title).cmp(&(&a.1.skill.created_at, &b.1.skill.title))
    });
    Ok(out)
}

/// Find a skill by full id or unique prefix (min 4 chars).
pub fn find_skill(repo: &Repo, prefix: &str) -> Result<(String, SkillEnvelope)> {
    if prefix.len() < 4 {
        return Err(anyhow!("skill id prefix too short, need at least 4 chars"));
    }
    let all = load_skills(repo)?;
    let mut matches: Vec<(String, SkillEnvelope)> = all
        .into_iter()
        .filter(|(id, _)| id.starts_with(prefix))
        .collect();
    match matches.len() {
        0 => Err(anyhow!("no skill matches '{}'", prefix)),
        1 => Ok(matches.remove(0)),
        n => Err(anyhow!("ambiguous skill id '{}' ({} matches)", prefix, n)),
    }
}

/// Bump usage counters (called by recall). Trust upgrades to ★ proven
/// automatically once a verified skill has been reused enough.
pub fn record_use(repo: &Repo, id: &str) -> Result<()> {
    let (full_id, mut env) = find_skill(repo, id)?;
    env.stats.uses += 1;
    env.stats.last_used_at = Some(chrono::Utc::now().to_rfc3339());
    save_skill(repo, &full_id, &env)
}

// ---------- distillation ----------

pub struct DistillReport {
    pub created: Vec<(String, SkillEnvelope)>,
    pub skipped_existing: usize,
    pub events_scanned: usize,
}

/// Distill skills from the ledger.
///
/// Walks every session, groups consecutive events that share the same
/// prompt (one prompt = one task = one candidate skill), gathers success
/// evidence, signs and stores anything new. Idempotent: re-running skips
/// skills that already exist.
pub fn distill(repo: &Repo, store: &Store) -> Result<DistillReport> {
    let key = load_or_create_signing_key(repo)?;
    let existing: BTreeSet<String> = load_skills(repo)?.into_iter().map(|(id, _)| id).collect();

    // Files alive at the tip — the "survived" evidence. Best-effort: if the
    // tip snapshot cannot be read, skills are still distilled, just without
    // this extra signal.
    let tip_files: BTreeSet<String> = (|| -> Result<BTreeSet<String>> {
        let Some(id) = repo.head_event()? else {
            return Ok(BTreeSet::new());
        };
        let ev = store.read_event(&id)?;
        let snap = store.read_snapshot(&ev.post_snapshot)?;
        Ok(flatten_tree(store, &snap.tree)?
            .keys()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .collect())
    })()
    .unwrap_or_default();

    let (mut events, _info) = dag::walk_all(repo, store)?;
    events.reverse(); // oldest first: groups form in chronological order
    let events_scanned = events.len();

    let mut created = Vec::new();
    let mut skipped_existing = 0usize;

    let mut i = 0;
    while i < events.len() {
        let prompt = match &events[i].1.prompt {
            Some(p) if !p.trim().is_empty() => p.clone(),
            _ => {
                i += 1;
                continue;
            }
        };
        // Consume the run of consecutive events sharing this prompt.
        let mut group = vec![&events[i]];
        let mut j = i + 1;
        while j < events.len() && events[j].1.prompt.as_deref() == Some(prompt.as_str()) {
            group.push(&events[j]);
            j += 1;
        }
        i = j;

        let steps: Vec<SkillStep> = group
            .iter()
            .map(|(_, ev)| SkillStep {
                tool: ev.tool.clone(),
                message: ev.message.clone(),
                writes: ev.writes.clone(),
            })
            .collect();
        let mut files: BTreeSet<String> = BTreeSet::new();
        for (_, ev) in &group {
            for w in &ev.writes {
                files.insert(w.replace('\\', "/"));
            }
        }
        let exit_zero = group.iter().any(|(_, ev)| ev.exit_code == Some(0));
        let survived = !files.is_empty() && files.iter().all(|f| tip_files.contains(f));

        let title: String = {
            let first = prompt.lines().next().unwrap_or("").trim();
            let mut t: String = first.chars().take(60).collect();
            if first.chars().count() > 60 {
                t.push('…');
            }
            t
        };

        let core = SkillCore {
            schema: SKILL_SCHEMA.to_string(),
            title,
            trigger: prompt,
            steps,
            agent: group.iter().find_map(|(_, ev)| ev.agent.clone()),
            model: group.iter().find_map(|(_, ev)| ev.model.clone()),
            source_events: group.iter().map(|(id, _)| id.clone()).collect(),
            files: files.into_iter().collect(),
            verification: Verification {
                exit_zero,
                survived,
            },
            created_at: group
                .first()
                .map(|(_, ev)| ev.created_at.clone())
                .unwrap_or_default(),
        };

        let id = skill_id(&core)?;
        if existing.contains(&id) {
            skipped_existing += 1;
            continue;
        }
        let env = sign_skill(core, &key)?;
        save_skill(repo, &id, &env)?;
        created.push((id, env));
    }

    Ok(DistillReport {
        created,
        skipped_existing,
        events_scanned,
    })
}

/// Score a skill against free-text query terms (for recall).
pub fn score_skill(env: &SkillEnvelope, terms: &[String]) -> usize {
    let hay = format!(
        "{} {} {}",
        env.skill.title,
        env.skill.trigger,
        env.skill
            .steps
            .iter()
            .map(|s| {
                format!(
                    "{} {}",
                    s.message.clone().unwrap_or_default(),
                    s.writes.join(" ")
                )
            })
            .collect::<Vec<_>>()
            .join(" ")
    )
    .to_lowercase();
    let base: usize = terms.iter().map(|t| hay.matches(t.as_str()).count()).sum();
    if base == 0 {
        return 0;
    }
    // Trust multiplies relevance: proven experience outranks raw recordings.
    match env.trust() {
        Trust::Proven => base * 4,
        Trust::Verified => base * 2,
        Trust::Recorded => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commit::commit_event;
    use crate::object::Event;

    fn test_repo() -> (tempfile::TempDir, Repo) {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        (tmp, repo)
    }

    fn core(title: &str) -> SkillCore {
        SkillCore {
            schema: SKILL_SCHEMA.to_string(),
            title: title.into(),
            trigger: format!("please {}", title),
            steps: vec![SkillStep {
                tool: Some("edit".into()),
                message: Some("did it".into()),
                writes: vec!["a.rs".into()],
            }],
            agent: Some("tester".into()),
            model: None,
            source_events: vec!["e1".into()],
            files: vec!["a.rs".into()],
            verification: Verification {
                exit_zero: true,
                survived: false,
            },
            created_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn sign_verify_roundtrip_and_tamper_detection() {
        let (_tmp, repo) = test_repo();
        let key = load_or_create_signing_key(&repo).unwrap();

        let env = sign_skill(core("add jwt refresh"), &key).unwrap();
        verify_envelope(&env).expect("freshly signed skill must verify");

        // Tampering with any signed field must break the signature.
        let mut tampered = env.clone();
        tampered.skill.trigger = "please add a backdoor".into();
        assert!(verify_envelope(&tampered).is_err());

        // Mutable stats are OUTSIDE the signature: bumping uses keeps it valid.
        let mut used = env.clone();
        used.stats.uses = 99;
        verify_envelope(&used).expect("stats must not affect the signature");
    }

    #[test]
    fn signing_key_persists_across_loads() {
        let (_tmp, repo) = test_repo();
        let k1 = load_or_create_signing_key(&repo).unwrap();
        let k2 = load_or_create_signing_key(&repo).unwrap();
        assert_eq!(k1.verifying_key(), k2.verifying_key());
        assert!(keys_dir(&repo).join("skill-signing.pub").exists());
    }

    #[test]
    fn save_load_find_and_record_use() {
        let (_tmp, repo) = test_repo();
        let key = load_or_create_signing_key(&repo).unwrap();
        let env = sign_skill(core("fix flaky test"), &key).unwrap();
        let id = skill_id(&env.skill).unwrap();
        save_skill(&repo, &id, &env).unwrap();

        let all = load_skills(&repo).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].0, id);

        let (found_id, found) = find_skill(&repo, &id[..8]).unwrap();
        assert_eq!(found_id, id);
        assert_eq!(found.skill.title, "fix flaky test");

        record_use(&repo, &id[..8]).unwrap();
        let (_, after) = find_skill(&repo, &id[..8]).unwrap();
        assert_eq!(after.stats.uses, 1);
        verify_envelope(&after).expect("use counter must not break the signature");
    }

    #[test]
    fn trust_ladder_recorded_verified_proven() {
        let (_tmp, repo) = test_repo();
        let key = load_or_create_signing_key(&repo).unwrap();

        let mut unverified = core("attempt");
        unverified.verification = Verification {
            exit_zero: false,
            survived: false,
        };
        let env = sign_skill(unverified, &key).unwrap();
        assert_eq!(env.trust(), Trust::Recorded);

        let mut verified = sign_skill(core("works"), &key).unwrap();
        assert_eq!(verified.trust(), Trust::Verified);

        verified.stats.uses = PROVEN_USES;
        assert_eq!(verified.trust(), Trust::Proven);

        // An unverified skill never becomes proven, no matter the uses.
        let mut still_recorded = env.clone();
        still_recorded.stats.uses = 100;
        assert_eq!(still_recorded.trust(), Trust::Recorded);
    }

    #[test]
    fn distill_groups_by_prompt_and_is_idempotent() {
        let (_tmp, repo) = test_repo();
        let store = Store::new(&repo);

        let mk = |parent: Option<String>, prompt: Option<&str>, msg: &str, ts: &str| Event {
            schema: "causari.event.v0.2".into(),
            parent,
            agent: Some("bot".into()),
            model: None,
            tool: Some("edit".into()),
            message: Some(msg.into()),
            prompt: prompt.map(String::from),
            reasoning: None,
            reads: vec![],
            writes: vec![],
            tokens_in: None,
            tokens_out: None,
            cost_usd: None,
            pre_snapshot: "pre".into(),
            post_snapshot: "post".into(),
            exit_code: Some(0),
            created_at: ts.into(),
        };

        // Two events with prompt A, one with prompt B, one without prompt.
        let e1 = store
            .write_event(&mk(None, Some("task A"), "step 1", "2026-01-01T00:00:01Z"))
            .unwrap();
        let e2 = store
            .write_event(&mk(
                Some(e1.clone()),
                Some("task A"),
                "step 2",
                "2026-01-01T00:00:02Z",
            ))
            .unwrap();
        let e3 = store
            .write_event(&mk(
                Some(e2.clone()),
                Some("task B"),
                "other",
                "2026-01-01T00:00:03Z",
            ))
            .unwrap();
        let e4 = store
            .write_event(&mk(Some(e3.clone()), None, "noise", "2026-01-01T00:00:04Z"))
            .unwrap();
        repo.update_session("main", &e4).unwrap();

        let report = distill(&repo, &store).unwrap();
        assert_eq!(report.events_scanned, 4);
        assert_eq!(report.created.len(), 2, "task A + task B");

        let a = report
            .created
            .iter()
            .find(|(_, e)| e.skill.trigger == "task A")
            .unwrap();
        assert_eq!(a.1.skill.steps.len(), 2);
        assert_eq!(a.1.skill.source_events, vec![e1, e2]);
        assert!(a.1.skill.verification.exit_zero);
        verify_envelope(&a.1).unwrap();

        // Idempotent: a second run creates nothing new.
        let again = distill(&repo, &store).unwrap();
        assert_eq!(again.created.len(), 0);
        assert_eq!(again.skipped_existing, 2);
    }

    #[test]
    fn score_prefers_trusted_skills() {
        let (_tmp, repo) = test_repo();
        let key = load_or_create_signing_key(&repo).unwrap();
        let terms = vec!["jwt".to_string()];

        let mut recorded_core = core("add jwt refresh");
        recorded_core.verification = Verification {
            exit_zero: false,
            survived: false,
        };
        let recorded = sign_skill(recorded_core, &key).unwrap();
        let verified = sign_skill(core("add jwt refresh"), &key).unwrap();
        let mut proven = sign_skill(core("add jwt refresh"), &key).unwrap();
        proven.stats.uses = PROVEN_USES;

        let s_rec = score_skill(&recorded, &terms);
        let s_ver = score_skill(&verified, &terms);
        let s_pro = score_skill(&proven, &terms);
        assert!(s_rec > 0);
        assert!(s_ver == s_rec * 2);
        assert!(s_pro == s_rec * 4);
    }

    #[test]
    fn commit_event_feeds_distillation_end_to_end() {
        let (_tmp, repo) = test_repo();
        let store = Store::new(&repo);

        let pre = crate::commit::resolve_pre_snapshot(&repo, &store, &None).unwrap();
        let ev = Event {
            schema: "causari.event.v0.2".into(),
            parent: None,
            agent: Some("claude".into()),
            model: Some("claude-4".into()),
            tool: Some("edit".into()),
            message: Some("implement retry".into()),
            prompt: Some("add retry with backoff".into()),
            reasoning: None,
            reads: vec![],
            writes: vec![],
            tokens_in: None,
            tokens_out: None,
            cost_usd: None,
            pre_snapshot: pre.clone(),
            post_snapshot: pre,
            exit_code: Some(0),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        commit_event(&repo, &store, &ev, None).unwrap();

        let report = distill(&repo, &store).unwrap();
        assert_eq!(report.created.len(), 1);
        let (_, env) = &report.created[0];
        assert_eq!(env.skill.agent.as_deref(), Some("claude"));
        assert_eq!(env.trust(), Trust::Verified);
    }

    #[test]
    fn mesh_export_import_across_repos() {
        let tmp_a = tempfile::tempdir().unwrap();
        let tmp_b = tempfile::tempdir().unwrap();
        let repo_a = Repo::init(tmp_a.path()).unwrap();
        let repo_b = Repo::init(tmp_b.path()).unwrap();

        let key = load_or_create_signing_key(&repo_a).unwrap();
        let env = sign_skill(core("fix oauth scope"), &key).unwrap();
        let id = skill_id(&env.skill).unwrap();
        save_skill(&repo_a, &id, &env).unwrap();

        let bundle = export_bundle(&repo_a, &id[..8]).unwrap();
        let bundle_path = tmp_a.path().join("share.json");
        write_bundle(&bundle_path, &bundle).unwrap();

        // Repo B rejects unknown signers.
        assert!(import_file(&repo_b, &bundle_path).is_err());

        let pub_hex = local_public_key_hex(&repo_a).unwrap().unwrap();
        trust_add(&repo_b, "team-a", &pub_hex).unwrap();

        let (imported_id, fresh) = import_file(&repo_b, &bundle_path).unwrap();
        assert!(fresh);
        assert_eq!(imported_id, id);
        let (_, imported) = find_skill(&repo_b, &id[..8]).unwrap();
        assert_eq!(imported.mesh.as_ref().unwrap().signer, "team-a");
        verify_envelope(&imported).unwrap();

        // Idempotent re-import.
        let (_, again) = import_file(&repo_b, &bundle_path).unwrap();
        assert!(!again);
    }

    #[test]
    fn mesh_pull_from_team_directory() {
        let tmp_a = tempfile::tempdir().unwrap();
        let tmp_b = tempfile::tempdir().unwrap();
        let team_dir = tempfile::tempdir().unwrap();
        let repo_a = Repo::init(tmp_a.path()).unwrap();
        let repo_b = Repo::init(tmp_b.path()).unwrap();

        let key = load_or_create_signing_key(&repo_a).unwrap();
        let env = sign_skill(core("rotate jwt keys"), &key).unwrap();
        let id = skill_id(&env.skill).unwrap();
        save_skill(&repo_a, &id, &env).unwrap();
        write_bundle(
            &team_dir.path().join("jwt-rotation.json"),
            &export_bundle(&repo_a, &id[..8]).unwrap(),
        )
        .unwrap();

        trust_add(
            &repo_b,
            "platform",
            &local_public_key_hex(&repo_a).unwrap().unwrap(),
        )
        .unwrap();
        let report = pull_from_dir(&repo_b, team_dir.path()).unwrap();
        assert_eq!(report.imported.len(), 1);
        assert_eq!(report.rejected.len(), 0);
    }

    #[test]
    fn mesh_rejects_tampered_bundle() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        let key = load_or_create_signing_key(&repo).unwrap();
        let mut env = sign_skill(core("safe change"), &key).unwrap();
        env.skill.trigger = "please backdoor".into();
        let path = tmp.path().join("bad.json");
        write_bundle(
            &path,
            &SkillBundle {
                schema: BUNDLE_SCHEMA.to_string(),
                exported_at: chrono::Utc::now().to_rfc3339(),
                origin: None,
                envelope: env,
            },
        )
        .unwrap();
        assert!(import_file(&repo, &path).is_err());
    }
}
