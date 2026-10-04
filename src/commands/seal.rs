use anyhow::{Context, Result};
use colored::Colorize;
use std::io::Write;
use std::path::PathBuf;

use crate::audit_seal::{self, VerifiedAudit};
use crate::cli::{SealArgs, SealCommand};
use crate::exit::exit_with;
use crate::repo::Repo;
use crate::seal;

pub fn run(args: SealArgs) -> Result<()> {
    match args.command {
        SealCommand::Verify { file, json } => verify(file, json),
        SealCommand::List { limit } => list(limit),
        SealCommand::Issuer => issuer(),
    }
}

fn verify(file: Option<PathBuf>, json: bool) -> Result<()> {
    if let Some(path) = file {
        return verify_file(&path, json);
    }

    let repo = Repo::discover()?;
    let count = seal::verify_chain(&repo)?;
    if json {
        println!(
            "{}",
            serde_json::json!({ "valid": true, "kind": "chain", "seals": count })
        );
        return Ok(());
    }
    if count == 0 {
        println!(
            "no seals issued yet — run {} or {} to start emitting receipts",
            "re proxy --seal".cyan(),
            "re audit --seal".cyan()
        );
        return Ok(());
    }
    println!(
        "{} {} seal(s) verified — every signature valid, chain contiguous from genesis",
        "✓".green().bold(),
        count.to_string().bold()
    );
    Ok(())
}

/// Verify one file: an audit seal bundle or a bare seal. Exit 0 when valid,
/// 1 when the file is a seal that does not verify, 2 when it is not a
/// seal at all (unreadable, not JSON, duplicate keys).
fn verify_file(path: &std::path::Path, json: bool) -> Result<()> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))
        .map_err(|e| exit_with(2, e))?;
    let value = seal::parse_json_strict(&raw)
        .context("parsing seal JSON")
        .map_err(|e| exit_with(2, e))?;

    if audit_seal::is_bundle(&value) {
        match audit_seal::verify_bundle(&value) {
            Ok(v) => {
                print_audit_verdict(&v, json);
                Ok(())
            }
            Err(e) => invalid(json, "audit", &format!("{e:#}")),
        }
    } else {
        match seal::verify_seal(&value) {
            Ok(()) => {
                print_seal_verdict(&value, json);
                Ok(())
            }
            Err(e) => invalid(json, "seal", &format!("{e:#}")),
        }
    }
}

/// Print the negative verdict on stdout (where `--json` consumers read it)
/// and end the process with status 1. Printing then returning an error
/// would report the reason twice.
fn invalid(json: bool, kind: &str, reason: &str) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::json!({ "valid": false, "kind": kind, "reason": reason })
        );
    } else {
        println!("{} invalid: {}", "✗".bold(), reason);
    }
    let _ = std::io::stdout().flush();
    std::process::exit(1);
}

fn pct(v: &serde_json::Value) -> String {
    match v.as_f64() {
        Some(r) => format!("{:.1}%", r * 100.0),
        None => "n/a".into(),
    }
}

fn print_audit_verdict(v: &VerifiedAudit, json: bool) {
    if json {
        let mut out = serde_json::to_value(v).unwrap_or_default();
        out["valid"] = serde_json::Value::Bool(true);
        out["kind"] = serde_json::Value::String("audit".into());
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return;
    }
    println!(
        "{} signature valid — {} (crovia.seal.v1, audit seal)",
        "✓".green().bold(),
        v.seal_id.cyan()
    );
    println!("  issuer    {}", v.issuer_id);
    println!("  pubkey    {}", v.pubkey_hex);
    println!(
        "  chain     sequence {}{}",
        v.sequence,
        match &v.prev_seal_hash {
            Some(h) => format!(", follows {}…", &h[..h.len().min(23)]),
            None => " (genesis)".to_string(),
        }
    );
    println!("  emitted   {}", v.emitted_at);
    println!("  repo      {}", v.repo);
    println!("  commit    {}", v.commit);
    println!(
        "  method    {}{}",
        v.method,
        if v.shallow {
            " · shallow clone: history truncated, figures partial"
        } else {
            ""
        }
    );
    if let Some(ver) = &v.generator_version {
        println!("  causari   {ver}");
    }
    let a = &v.audit;
    println!(
        "  audit     {} commits · AI-tagged (metadata matched): {} commits, {} introduced, {} survived ({} line-weighted · {} capped · median {})",
        a["total_commits"],
        a["verified"]["commits"],
        a["verified"]["introduced"],
        a["verified"]["surviving"],
        pct(&a["verified"]["survival_rate"]),
        pct(&a["verified"]["capped_survival_rate"]),
        pct(&a["verified"]["median_survival"]),
    );
    println!(
        "  {} the issuer key signed this exact audit JSON for this commit with this method; the numbers were not altered since. It does not prove they are true — rerun `re audit` on the commit to check.",
        "means:".bright_black()
    );
}

fn print_seal_verdict(value: &serde_json::Value, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "valid": true,
                "kind": "seal",
                "seal_id": value["seal_id"],
                "issuer_id": value["issuer"]["id"],
                "pubkey_hex": value["issuer"]["pubkey"]["key_hex"],
                "sequence": value["chain"]["sequence"],
                "prev_seal_hash": value["chain"]["prev_seal_hash"],
                "emitted_at": value["timestamp"]["emitted_at"],
                "generator": value["generator"],
                "subject": value["subject"],
            }))
            .unwrap_or_default()
        );
        return;
    }
    println!(
        "{} signature valid — {} (crovia.seal.v1)",
        "✓".green().bold(),
        value["seal_id"].as_str().unwrap_or("(seal)").cyan()
    );
    println!("  structure: conformant (typed fields, no duplicate keys, CSC-1 canonical)");
    println!(
        "  issuer:    {} · key {}…",
        value["issuer"]["id"].as_str().unwrap_or("?"),
        value["issuer"]["pubkey"]["key_hex"]
            .as_str()
            .map(|k| &k[..k.len().min(16)])
            .unwrap_or("?")
    );
    println!(
        "  {} a valid signature proves who issued the receipt and that it was not altered; it does not by itself prove the trust of that key or the completeness of the chain.",
        "note:".bright_black()
    );
}

fn list(limit: usize) -> Result<()> {
    let repo = Repo::discover()?;
    let path = seal::seals_log_path(&repo);
    if !path.exists() {
        println!(
            "no seals issued yet — run {} or {} to start emitting receipts",
            "re proxy --seal".cyan(),
            "re audit --seal".cyan()
        );
        return Ok(());
    }
    let raw = std::fs::read_to_string(&path)?;
    let seals: Vec<serde_json::Value> = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| seal::parse_json_strict(l).ok())
        .collect();

    let total = seals.len();
    for s in seals.iter().rev().take(limit) {
        let id = s["seal_id"].as_str().unwrap_or("?");
        let seq = s["chain"]["sequence"].as_u64().unwrap_or(0);
        let params = &s["generator"]["params"];
        let what = if params["subject_type"].as_str() == Some(audit_seal::SUBJECT_TYPE) {
            let commit = params["commit"].as_str().unwrap_or("?");
            format!("audit @ {}", &commit[..commit.len().min(10)])
        } else {
            s["generator"]["id"].as_str().unwrap_or("?").to_string()
        };
        let at = s["timestamp"]["emitted_at"].as_str().unwrap_or("?");
        let out_len = s["subject"]["output_len"].as_u64().unwrap_or(0);
        println!(
            "  {} {}  {}  {}  {}",
            format!("#{:<4}", seq).bright_black(),
            id.cyan(),
            what.bold(),
            format!("{}B out", out_len).bright_black(),
            at.bright_black()
        );
    }
    if total > limit {
        println!("  … {} more (use -n to show more)", total - limit);
    }
    Ok(())
}

fn issuer() -> Result<()> {
    let repo = Repo::discover()?;
    // Read-only: printing an identity must not mint one.
    let Some(key) = crate::keys::load(&repo, "seal-issuer")? else {
        println!("no seal issuer key yet");
        println!(
            "  one is created (owner-readable only) the first time you run {} or {}",
            "re proxy --seal".cyan(),
            "re audit --seal".cyan()
        );
        return Ok(());
    };
    let issuer = seal::SealIssuer::load_or_create(&repo, None)?;
    println!("issuer id   {}", issuer.issuer_id().cyan());
    println!(
        "pubkey      {}",
        hex::encode(key.verifying_key().to_bytes())
    );
    println!("key file    .causari/keys/seal-issuer.key (0600)");
    println!("next seq    {}", issuer.sequence());
    println!();
    println!(
        "Share the pubkey: anyone can verify your seals offline with it — \
         no server, no account, no Crovia involvement required."
    );
    Ok(())
}
