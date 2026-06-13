use anyhow::Result;
use colored::Colorize;
use std::path::{Path, PathBuf};

use crate::cli::{ProofArgs, ProofCommand};
use crate::proof;
use crate::repo::Repo;
use crate::store::Store;

const DEFAULT_PROOF: &str = "causari-proof.json";
const DEFAULT_BADGE: &str = "causari-proof.svg";

/// `re proof` — the viral, trustless provenance certificate.
///
/// - `re proof generate`  sign the repo's provenance + emit an embeddable badge
/// - `re proof verify`    check a proof offline (anyone, anywhere, no server)
pub fn run(args: ProofArgs) -> Result<()> {
    match args.command {
        ProofCommand::Generate {
            output,
            badge,
            no_badge,
        } => generate(output, badge, no_badge),
        ProofCommand::Verify { file, against_repo } => verify(file, against_repo),
    }
}

fn generate(output: Option<PathBuf>, badge: Option<PathBuf>, no_badge: bool) -> Result<()> {
    let repo = Repo::discover()?;
    let store = Store::new(&repo);

    let env = proof::generate(&repo, &store)?;
    let out = output.unwrap_or_else(|| PathBuf::from(DEFAULT_PROOF));
    proof::write_proof_file(&out, &env)?;

    println!(
        "{} {}",
        "proof:".green().bold(),
        out.display().to_string().cyan()
    );
    let m = &env.manifest;
    println!(
        "  {} events · {} session(s) · {} file(s)",
        m.events, m.sessions, m.files_touched
    );
    if !m.agents.is_empty() {
        println!("  agents:  {}", m.agents.join(", "));
    }
    if !m.models.is_empty() {
        println!("  models:  {}", m.models.join(", "));
    }
    println!(
        "  skills:  {} total ({} verified, {} proven)",
        m.skills.total, m.skills.verified, m.skills.proven
    );
    println!("  signer:  {}", (&env.public_key[..16]).bright_black());

    if !no_badge {
        let badge_path = badge.unwrap_or_else(|| PathBuf::from(DEFAULT_BADGE));
        std::fs::write(&badge_path, proof::badge_svg(&env))?;
        println!();
        println!(
            "  {} {}",
            "badge:".green().bold(),
            badge_path.display().to_string().cyan()
        );
        println!("  paste into your README:");
        println!("    {}", proof::badge_markdown(&env).bright_black());
    }
    println!();
    println!(
        "  {} anyone can check it with {} — no server, no account.",
        "trustless:".bright_black(),
        "re proof verify".cyan()
    );
    Ok(())
}

fn verify(file: Option<PathBuf>, against_repo: bool) -> Result<()> {
    let path = file.unwrap_or_else(|| PathBuf::from(DEFAULT_PROOF));
    let env = proof::read_proof_file(&path)?;

    proof::verify_signature(&env).map_err(|e| {
        anyhow::anyhow!(
            "{} {}",
            "INVALID".red().bold(),
            e
        )
    })?;

    println!(
        "{} signature valid (Ed25519)",
        "ok".green().bold(),
    );
    let m = &env.manifest;
    println!("  repo:    {}", m.repo);
    println!("  created: {}", m.generated_at);
    println!(
        "  attests: {} events · {} agents · {} proven skill(s)",
        m.events,
        m.agents.len(),
        m.skills.proven
    );
    println!("  signer:  {}", (&env.public_key[..16]).bright_black());

    if against_repo {
        check_against_repo(&path, &env)?;
    }
    Ok(())
}

fn check_against_repo(_path: &Path, env: &proof::ProofEnvelope) -> Result<()> {
    let repo = Repo::discover()?;
    let store = Store::new(&repo);
    if proof::matches_repo(&repo, &store, env)? {
        println!(
            "  {} proof matches the current ledger exactly",
            "fresh:".green().bold()
        );
        Ok(())
    } else {
        println!(
            "  {} ledger has changed since this proof was generated — re-run {}",
            "stale:".yellow().bold(),
            "re proof generate".cyan()
        );
        Ok(())
    }
}
