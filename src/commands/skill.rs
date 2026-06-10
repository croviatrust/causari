use anyhow::Result;
use colored::Colorize;

use crate::cli::{SkillArgs, SkillCommand};
use crate::repo::Repo;
use crate::skill::{self, Trust};
use crate::store::Store;

/// `re skill` — the experience layer at the command line.
///
/// - `re skill distill`      events → signed skills (idempotent)
/// - `re skill list`         every skill with its trust badge
/// - `re skill show <id>`    trigger, steps, evidence, signature
/// - `re skill verify [id]`  Ed25519 check: tampered skills are exposed
pub fn run(args: SkillArgs) -> Result<()> {
    match args.command {
        SkillCommand::Distill => distill(),
        SkillCommand::List => list(),
        SkillCommand::Show { id } => show(&id),
        SkillCommand::Verify { id } => verify(id.as_deref()),
    }
}

fn trust_colored(t: Trust) -> colored::ColoredString {
    match t {
        Trust::Recorded => format!("{} recorded", t.badge()).bright_black(),
        Trust::Verified => format!("{} verified", t.badge()).green(),
        Trust::Proven => format!("{} proven", t.badge()).yellow().bold(),
    }
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
            "  {} skills are signed with this repo's Ed25519 key — `re skill verify` any time.",
            "note:".bright_black()
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
            "  {} {}  {}  {} use(s)  {} file(s)",
            trust_colored(env.trust()),
            (&id[..10]).yellow(),
            env.skill.title,
            env.stats.uses,
            env.skill.files.len()
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
    println!("  title:      {}", env.skill.title.bold());
    if let Some(a) = &env.skill.agent {
        println!("  agent:      {}", a.cyan());
    }
    if let Some(m) = &env.skill.model {
        println!("  model:      {}", m.cyan());
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
    println!();
    println!("  {}", "steps:".bright_black().italic());
    for (i, step) in env.skill.steps.iter().enumerate() {
        println!(
            "    {}. [{}] {}{}",
            i + 1,
            step.tool.as_deref().unwrap_or("-").cyan(),
            step.message.as_deref().unwrap_or(""),
            if step.writes.is_empty() {
                String::new()
            } else {
                format!("  → {}", step.writes.join(", "))
                    .bright_black()
                    .to_string()
            }
        );
    }
    if !env.skill.files.is_empty() {
        println!();
        println!("  files:      {}", env.skill.files.join(", "));
    }
    println!(
        "  events:     {}",
        env.skill
            .source_events
            .iter()
            .map(|e| e[..10.min(e.len())].to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
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
