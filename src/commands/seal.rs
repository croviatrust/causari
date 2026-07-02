use anyhow::{Context, Result};
use colored::Colorize;

use crate::cli::{SealArgs, SealCommand};
use crate::repo::Repo;
use crate::seal;

pub fn run(args: SealArgs) -> Result<()> {
    match args.command {
        SealCommand::Verify { file } => verify(file),
        SealCommand::List { limit } => list(limit),
        SealCommand::Issuer => issuer(),
    }
}

fn verify(file: Option<std::path::PathBuf>) -> Result<()> {
    if let Some(path) = file {
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let value: serde_json::Value = serde_json::from_str(&raw).context("parsing seal JSON")?;
        seal::verify_seal(&value)?;
        println!(
            "{} {} — signature valid, structure conformant (crovia.seal.v1)",
            "✓".green().bold(),
            value["seal_id"].as_str().unwrap_or("(seal)").cyan()
        );
        return Ok(());
    }

    let repo = Repo::discover()?;
    let count = seal::verify_chain(&repo)?;
    if count == 0 {
        println!(
            "no seals issued yet — run {} to start emitting receipts",
            "re proxy --seal".cyan()
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

fn list(limit: usize) -> Result<()> {
    let repo = Repo::discover()?;
    let path = seal::seals_log_path(&repo);
    if !path.exists() {
        println!(
            "no seals issued yet — run {} to start emitting receipts",
            "re proxy --seal".cyan()
        );
        return Ok(());
    }
    let raw = std::fs::read_to_string(&path)?;
    let seals: Vec<serde_json::Value> = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();

    let total = seals.len();
    for s in seals.iter().rev().take(limit) {
        let id = s["seal_id"].as_str().unwrap_or("?");
        let seq = s["chain"]["sequence"].as_u64().unwrap_or(0);
        let model = s["generator"]["id"].as_str().unwrap_or("?");
        let at = s["timestamp"]["emitted_at"].as_str().unwrap_or("?");
        let out_len = s["subject"]["output_len"].as_u64().unwrap_or(0);
        println!(
            "  {} {}  {}  {}  {}",
            format!("#{:<4}", seq).bright_black(),
            id.cyan(),
            model.bold(),
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
    let issuer = seal::SealIssuer::load_or_create(&repo, None)?;
    println!("issuer id   {}", "urn:crovia:seal-issuer:causari".cyan());
    println!("pubkey      {}", issuer.pubkey_hex());
    println!("next seq    {}", issuer.sequence());
    println!();
    println!(
        "Share the pubkey: anyone can verify your seals offline with it — \
         no server, no account, no Crovia involvement required."
    );
    Ok(())
}
