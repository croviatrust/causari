use anyhow::{Context, Result, anyhow};
use ed25519_dalek::{Signature, Signer, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

use crate::dag;
use crate::object::{canonical_json, hash_bytes};
use crate::repo::Repo;
use crate::skill::{self, Trust};
use crate::store::Store;

// CAUSARI PROOF — verifiable AI-provenance certificate.
//
// A proof is a signed, self-contained summary of everything Causari has
// recorded for a repository: how many agent actions, which agents and models,
// how much verified experience, and a digest that binds it all together.
//
// The point is *trustless distribution*. The proof is signed with the repo's
// Ed25519 key; anyone — a reviewer, an auditor, a stranger on the internet —
// can run `re proof verify proof.json` and confirm it was not altered, with
// no server, no account, no trust in Causari the company.
//
// That is the viral surface: a repo drops a "AI provenance: verified" badge
// in its README, every visitor sees it, and the proof behind it checks out
// offline. Generation and verification are free forever. The hosted public
// verification page, the org-wide proof registry and RFC 3161 anchoring are
// the commercial Trust Plane on top.

pub const PROOF_SCHEMA: &str = "causari.proof.v0.1";

/// Aggregate skill trust counts.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillSummary {
    pub total: usize,
    pub verified: usize,
    pub proven: usize,
}

/// The signed core of a proof. Deterministic: same ledger → same manifest
/// (modulo `generated_at`, which is excluded from the binding digest).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProofManifest {
    pub schema: String,
    /// Human hint (repo directory name). Not a security boundary.
    pub repo: String,
    pub generated_at: String,
    /// HEAD event id at generation time, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    pub events: usize,
    pub sessions: usize,
    /// Distinct agent identifiers seen across the ledger, sorted.
    pub agents: Vec<String>,
    /// Distinct model identifiers seen across the ledger, sorted.
    pub models: Vec<String>,
    pub files_touched: usize,
    pub skills: SkillSummary,
    /// BLAKE3 over the sorted event id set — binds the proof to the exact
    /// ledger contents. Recomputable from any clone to detect drift.
    pub ledger_digest: String,
}

/// A proof manifest plus its detached Ed25519 signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofEnvelope {
    pub manifest: ProofManifest,
    /// hex Ed25519 public key (32 bytes)
    pub public_key: String,
    /// hex Ed25519 signature over canonical_json(manifest) (64 bytes)
    pub signature: String,
}

/// Digest that binds a proof to exact ledger contents: BLAKE3 of the sorted,
/// newline-joined event ids. Independent of `generated_at`, so two clones of
/// the same history produce the same digest.
fn compute_ledger_digest(event_ids: &BTreeSet<String>) -> String {
    let joined = event_ids.iter().cloned().collect::<Vec<_>>().join("\n");
    hash_bytes(joined.as_bytes())
}

/// Build the manifest from the current repository state.
pub fn build_manifest(repo: &Repo, store: &Store) -> Result<ProofManifest> {
    let (events, _info) = dag::walk_all(repo, store)?;

    let mut agents: BTreeSet<String> = BTreeSet::new();
    let mut models: BTreeSet<String> = BTreeSet::new();
    let mut files: BTreeSet<String> = BTreeSet::new();
    let mut ids: BTreeSet<String> = BTreeSet::new();
    for (id, ev) in &events {
        ids.insert(id.clone());
        if let Some(a) = &ev.agent {
            agents.insert(a.clone());
        }
        if let Some(m) = &ev.model {
            models.insert(m.clone());
        }
        for w in &ev.writes {
            files.insert(w.replace('\\', "/"));
        }
    }

    let mut summary = SkillSummary::default();
    for (_, env) in skill::load_skills(repo)? {
        if skill::verify_envelope(&env).is_err() {
            continue;
        }
        summary.total += 1;
        match env.trust() {
            Trust::Proven => summary.proven += 1,
            Trust::Verified => summary.verified += 1,
            Trust::Recorded => {}
        }
    }

    Ok(ProofManifest {
        schema: PROOF_SCHEMA.to_string(),
        repo: repo
            .root
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("repo")
            .to_string(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        head: repo.head_event()?,
        events: events.len(),
        sessions: dag::list_sessions(repo)?.len(),
        agents: agents.into_iter().collect(),
        models: models.into_iter().collect(),
        files_touched: files.len(),
        skills: summary,
        ledger_digest: compute_ledger_digest(&ids),
    })
}

/// Sign a manifest with the repo's Ed25519 key (the same identity used for
/// skills — a repo speaks with one voice).
pub fn sign_manifest(repo: &Repo, manifest: ProofManifest) -> Result<ProofEnvelope> {
    let key = skill::load_or_create_signing_key(repo)?;
    let msg = canonical_json(&manifest)?;
    let sig = key.sign(&msg);
    Ok(ProofEnvelope {
        manifest,
        public_key: hex::encode(key.verifying_key().to_bytes()),
        signature: hex::encode(sig.to_bytes()),
    })
}

/// Generate a fresh, signed proof for the repository.
pub fn generate(repo: &Repo, store: &Store) -> Result<ProofEnvelope> {
    let manifest = build_manifest(repo, store)?;
    sign_manifest(repo, manifest)
}

/// Verify the signature of a proof envelope. Ok means: signed by the embedded
/// key, unaltered since. This needs no repository and no network.
pub fn verify_signature(env: &ProofEnvelope) -> Result<()> {
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
    let msg = canonical_json(&env.manifest)?;
    vk.verify(&msg, &sig)
        .map_err(|_| anyhow!("signature verification FAILED — proof was modified after signing"))
}

/// Does this proof still match the live repository? Recomputes the ledger
/// digest and compares. A mismatch means the proof is stale (new events since
/// it was generated) or describes a different repo.
pub fn matches_repo(repo: &Repo, store: &Store, env: &ProofEnvelope) -> Result<bool> {
    let (events, _) = dag::walk_all(repo, store)?;
    let ids: BTreeSet<String> = events.into_iter().map(|(id, _)| id).collect();
    Ok(compute_ledger_digest(&ids) == env.manifest.ledger_digest)
}

pub fn read_proof_file(path: &Path) -> Result<ProofEnvelope> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading proof {}", path.display()))?;
    serde_json::from_str(&raw).context("parsing proof JSON")
}

pub fn write_proof_file(path: &Path, env: &ProofEnvelope) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(path, serde_json::to_string_pretty(env)?)?;
    Ok(())
}

/// A self-contained SVG badge: "AI provenance · verified by Causari".
/// No external assets, embeddable anywhere.
pub fn badge_svg(env: &ProofEnvelope) -> String {
    let n = env.manifest.events;
    let right = format!("{} events ✓", n);
    // Width scales loosely with the right-hand text length.
    let rw = 70 + right.chars().count() as i32 * 6;
    let total = 132 + rw;
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{total}" height="20" role="img" aria-label="AI provenance: {right}">
  <linearGradient id="s" x2="0" y2="100%"><stop offset="0" stop-color="#bbb" stop-opacity=".1"/><stop offset="1" stop-opacity=".1"/></linearGradient>
  <rect rx="3" width="{total}" height="20" fill="#555"/>
  <rect rx="3" x="132" width="{rw}" height="20" fill="#7C3AED"/>
  <rect rx="3" width="{total}" height="20" fill="url(#s)"/>
  <g fill="#fff" text-anchor="middle" font-family="Verdana,Geneva,sans-serif" font-size="11">
    <text x="66" y="14">AI provenance</text>
    <text x="{tx}" y="14">{right}</text>
  </g>
</svg>"##,
        total = total,
        rw = rw,
        right = right,
        tx = 132 + rw / 2,
    )
}

/// Markdown badge snippet pointing at the (paid, hosted) public verifier,
/// with the local SVG as the image. Offline verification stays `re proof
/// verify`.
pub fn badge_markdown(env: &ProofEnvelope) -> String {
    let pk = &env.public_key[..16.min(env.public_key.len())];
    format!(
        "[![AI provenance — verified by Causari](causari-proof.svg)](https://causari.dev/verify?k={})",
        pk
    )
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

    fn record(repo: &Repo, store: &Store, agent: &str, model: &str, file: &str) {
        let pre = crate::commit::resolve_pre_snapshot(repo, store, &repo.head_event().unwrap())
            .unwrap();
        let ev = Event {
            schema: "causari.event.v0.2".into(),
            parent: repo.head_event().unwrap(),
            agent: Some(agent.into()),
            model: Some(model.into()),
            tool: Some("edit".into()),
            message: Some(format!("touch {}", file)),
            prompt: Some(format!("please edit {}", file)),
            reasoning: None,
            reads: vec![],
            writes: vec![file.into()],
            tokens_in: None,
            tokens_out: None,
            cost_usd: None,
            pre_snapshot: pre.clone(),
            post_snapshot: pre,
            exit_code: Some(0),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        commit_event(repo, store, &ev, None).unwrap();
    }

    #[test]
    fn empty_repo_proof_verifies() {
        let (_t, repo) = test_repo();
        let store = Store::new(&repo);
        let env = generate(&repo, &store).unwrap();
        verify_signature(&env).expect("empty proof must verify");
        assert_eq!(env.manifest.events, 0);
        assert!(matches_repo(&repo, &store, &env).unwrap());
    }

    #[test]
    fn proof_aggregates_agents_models_and_files() {
        let (_t, repo) = test_repo();
        let store = Store::new(&repo);
        record(&repo, &store, "claude", "claude-4", "a.rs");
        record(&repo, &store, "cursor", "gpt-5", "b.rs");
        record(&repo, &store, "claude", "claude-4", "a.rs"); // dup agent/model/file

        let env = generate(&repo, &store).unwrap();
        verify_signature(&env).unwrap();
        assert_eq!(env.manifest.events, 3);
        assert_eq!(env.manifest.agents, vec!["claude", "cursor"]);
        assert_eq!(env.manifest.models, vec!["claude-4", "gpt-5"]);
        assert_eq!(env.manifest.files_touched, 2);
    }

    #[test]
    fn tampering_breaks_the_signature() {
        let (_t, repo) = test_repo();
        let store = Store::new(&repo);
        record(&repo, &store, "claude", "claude-4", "a.rs");
        let mut env = generate(&repo, &store).unwrap();

        verify_signature(&env).unwrap();
        // Inflate the event count to fake more provenance than exists.
        env.manifest.events = 9999;
        assert!(verify_signature(&env).is_err());
    }

    #[test]
    fn digest_detects_stale_proof_after_new_events() {
        let (_t, repo) = test_repo();
        let store = Store::new(&repo);
        record(&repo, &store, "claude", "claude-4", "a.rs");
        let env = generate(&repo, &store).unwrap();
        assert!(matches_repo(&repo, &store, &env).unwrap());

        record(&repo, &store, "claude", "claude-4", "c.rs");
        assert!(
            !matches_repo(&repo, &store, &env).unwrap(),
            "proof must read as stale once new events land"
        );
    }

    #[test]
    fn proof_file_roundtrip_and_offline_verify() {
        let (_t, repo) = test_repo();
        let store = Store::new(&repo);
        record(&repo, &store, "claude", "claude-4", "a.rs");
        let env = generate(&repo, &store).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("causari-proof.json");
        write_proof_file(&path, &env).unwrap();

        // Verifiable with no repo in sight — trustless distribution.
        let loaded = read_proof_file(&path).unwrap();
        verify_signature(&loaded).unwrap();
        assert_eq!(loaded.manifest.ledger_digest, env.manifest.ledger_digest);
    }

    #[test]
    fn badge_svg_is_self_contained() {
        let (_t, repo) = test_repo();
        let store = Store::new(&repo);
        record(&repo, &store, "claude", "claude-4", "a.rs");
        let env = generate(&repo, &store).unwrap();
        let svg = badge_svg(&env);
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("AI provenance"));
        // No external asset fetches: the badge embeds nothing it must download.
        assert!(!svg.contains("<image"));
        assert!(!svg.contains("xlink:href"));
        assert!(!svg.to_lowercase().contains("https://"));
    }
}
