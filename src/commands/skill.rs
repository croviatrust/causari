use anyhow::Result;
use colored::Colorize;
use std::io::Write;
use std::path::Path;

use crate::cli::{SkillArgs, SkillCommand, SkillTrustCommand};
use crate::repo::Repo;
use crate::skill::{self, Trust};
use crate::store::Store;

/// `re skill` — the experience layer + trust plane at the command line.
pub fn run(args: SkillArgs) -> Result<()> {
    match args.command {
        SkillCommand::Distill => distill(),
        SkillCommand::List => list(),
        SkillCommand::Show { id } => show(&id),
        SkillCommand::Verify { id } => verify(id.as_deref()),
        SkillCommand::Export { id, output } => export(&id, output.as_deref()),
        SkillCommand::Import { file } => import(&file),
        SkillCommand::Pull { dir } => pull(&dir),
        SkillCommand::Trust { command } => trust(command),
    }
}

fn trust_colored(t: Trust) -> colored::ColoredString {
    match t {
        Trust::Recorded => format!("{} recorded", t.badge()).bright_black(),
        Trust::Verified => format!("{} verified", t.badge()).green(),
        Trust::Proven => format!("{} proven", t.badge()).yellow().bold(),
    }
}

fn signer_tag(env: &skill::SkillEnvelope) -> String {
    env.mesh
        .as_ref()
        .map(|m| format!("[{}]", m.signer))
        .unwrap_or_else(|| "[local]".to_string())
}

fn distill() -> Result<()> {
    let repo = Repo::discover()?;
    let store = Store::new(&repo);

    let report = skill::distill(&repo, &store)?;
    println!(
        "{} {} event(s) scanned, {} new skill(s), {} already distilled",
        "distill:".green().bold(),
        report.events_scanned,
        report.created.len(),
        report.skipped_existing
    );
    for (id, env) in &report.created {
        println!(
            "  {} {}  {}",
            trust_colored(env.trust()),
            (&id[..10]).yellow(),
            env.skill.title
        );
    }
    if !report.created.is_empty() {
        println!();
        println!(
            "  {} share with your team: {} then {}",
            "tip:".bright_black(),
            "re skill export <id>".cyan(),
            "re skill trust pubkey".cyan()
        );
    }
    Ok(())
}

fn list() -> Result<()> {
    let repo = Repo::discover()?;
    let skills = skill::load_skills(&repo)?;
    if skills.is_empty() {
        println!(
            "{} no skills yet — run {} first",
            "skills:".yellow().bold(),
            "re skill distill".cyan()
        );
        return Ok(());
    }
    println!("{} {} skill(s)", "skills:".green().bold(), skills.len());
    for (id, env) in &skills {
        println!(
            "  {} {} {}  {}  {} use(s)",
            trust_colored(env.trust()),
            (&id[..10]).yellow(),
            signer_tag(env).bright_black(),
            env.skill.title,
            env.stats.uses,
        );
    }
    Ok(())
}

fn show(id: &str) -> Result<()> {
    let repo = Repo::discover()?;
    let (full_id, env) = skill::find_skill(&repo, id)?;
    let sig_ok = skill::verify_envelope(&env).is_ok();

    println!("{} {}", "skill".yellow().bold(), (&full_id[..16]).yellow());
    println!("  trust:      {}", trust_colored(env.trust()));
    println!(
        "  signature:  {}",
        if sig_ok {
            "valid (Ed25519)".green()
        } else {
            "INVALID — content was modified after signing".red().bold()
        }
    );
    if let Some(mesh) = &env.mesh {
        println!("  signer:     {}", mesh.signer.cyan());
        if let Some(from) = &mesh.imported_from {
            println!("  imported:   {}", from.bright_black());
        }
    }
    println!("  title:      {}", env.skill.title.bold());
    if let Some(a) = &env.skill.agent {
        println!("  agent:      {}", a.cyan());
    }
    println!("  created:    {}", env.skill.created_at);
    println!(
        "  evidence:   exit_zero={} survived={}",
        env.skill.verification.exit_zero, env.skill.verification.survived
    );
    println!(
        "  uses:       {}{}",
        env.stats.uses,
        env.stats
            .last_used_at
            .as_deref()
            .map(|t| format!("  (last {})", t))
            .unwrap_or_default()
    );
    println!();
    println!("  {}", "trigger:".bright_black().italic());
    for line in env.skill.trigger.lines() {
        println!("    {}", line);
    }
    Ok(())
}

fn verify(id: Option<&str>) -> Result<()> {
    let repo = Repo::discover()?;
    let targets: Vec<(String, skill::SkillEnvelope)> = match id {
        Some(prefix) => vec![skill::find_skill(&repo, prefix)?],
        None => skill::load_skills(&repo)?,
    };
    if targets.is_empty() {
        println!(
            "{} nothing to verify (no skills)",
            "verify:".yellow().bold()
        );
        return Ok(());
    }

    let mut bad = 0usize;
    for (full_id, env) in &targets {
        match skill::verify_envelope(env) {
            Ok(()) => println!(
                "  {} {}  {}",
                "ok".green().bold(),
                (&full_id[..10]).yellow(),
                env.skill.title
            ),
            Err(e) => {
                bad += 1;
                println!(
                    "  {} {}  {}  — {}",
                    "FAIL".red().bold(),
                    (&full_id[..10]).yellow(),
                    env.skill.title,
                    e
                );
            }
        }
    }
    println!();
    if bad == 0 {
        println!(
            "{} {} skill(s), every signature valid",
            "verify:".green().bold(),
            targets.len()
        );
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "{} of {} skill(s) failed signature verification",
            bad,
            targets.len()
        ))
    }
}

fn export(id: &str, output: Option<&Path>) -> Result<()> {
    let repo = Repo::discover()?;
    let bundle = skill::export_bundle(&repo, id)?;
    let json = serde_json::to_string_pretty(&bundle)?;

    match output {
        Some(path) => {
            skill::write_bundle(path, &bundle)?;
            println!(
                "{} {} → {}",
                "exported".green().bold(),
                (&skill::skill_id(&bundle.envelope.skill)?[..10]).yellow(),
                path.display()
            );
        }
        None => {
            print!("{}", json);
            std::io::stdout().flush()?;
        }
    }
    Ok(())
}

fn import(file: &Path) -> Result<()> {
    let repo = Repo::discover()?;
    let (id, fresh) = skill::import_file(&repo, file)?;
    if fresh {
        let (_, env) = skill::find_skill(&repo, &id)?;
        println!(
            "{} {} {}  {}",
            "imported".green().bold(),
            (&id[..10]).yellow(),
            signer_tag(&env).bright_black(),
            env.skill.title
        );
    } else {
        println!(
            "{} {} already present (signature unchanged)",
            "skipped:".bright_black(),
            (&id[..10]).yellow()
        );
    }
    Ok(())
}

fn pull(dir: &Path) -> Result<()> {
    let repo = Repo::discover()?;
    let report = skill::pull_from_dir(&repo, dir)?;
    println!(
        "{} {} imported, {} already present, {} rejected",
        "pull:".green().bold(),
        report.imported.len(),
        report.skipped_existing,
        report.rejected.len()
    );
    for (id, env) in &report.imported {
        println!(
            "  + {} {}  {}",
            (&id[..10]).yellow(),
            signer_tag(env).bright_black(),
            env.skill.title
        );
    }
    for (name, err) in &report.rejected {
        println!("  {} {}  {}", "✗".red(), name.bright_black(), err);
    }
    Ok(())
}

fn trust(command: SkillTrustCommand) -> Result<()> {
    let repo = Repo::discover()?;
    match command {
        SkillTrustCommand::Pubkey => match skill::local_public_key_hex(&repo)? {
            Some(hex) => {
                println!("{}", "pubkey:".green().bold());
                println!("  {}", hex.cyan());
                println!();
                println!(
                    "  Teammates run: {}",
                    format!("re skill trust add you {}", hex).bright_black()
                );
            }
            None => {
                println!(
                    "{} no signing key yet — run {} first",
                    "pubkey:".yellow().bold(),
                    "re skill distill".cyan()
                );
            }
        },
        SkillTrustCommand::Add { label, key } => {
            skill::trust_add(&repo, &label, &key)?;
            println!("{} trusted key {}", "added".green().bold(), label.cyan());
        }
        SkillTrustCommand::List => {
            let local = skill::local_public_key_hex(&repo)?;
            let trusted = skill::list_trusted_keys(&repo)?;
            println!("{}", "trust:".green().bold());
            if let Some(hex) = local {
                println!("  {} (this repo)  {}", "local".cyan(), hex.bright_black());
            }
            if trusted.is_empty() {
                println!("  {} no org keys trusted yet", "(none)".bright_black());
            } else {
                for (label, hex) in trusted {
                    println!("  {}  {}", label.cyan(), hex.bright_black());
                }
            }
        }
        SkillTrustCommand::Remove { label } => {
            skill::trust_remove(&repo, &label)?;
            println!("{} {}", "removed".green().bold(), label.cyan());
        }
    }
    Ok(())
}
