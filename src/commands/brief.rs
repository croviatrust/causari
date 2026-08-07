/// `re brief` — portable experience briefing for any model.
///
/// Emits a Markdown block of trust-ranked, Ed25519-signed experience relevant
/// to a task, ready to inject into any agent's context: CLAUDE.md, AGENTS.md,
/// .cursorrules, a system prompt, or a plain pipe. This is how a lesson
/// learned while working with one model survives into the next one — the
/// experience lives in Causari; the model is interchangeable.
use anyhow::Result;

use crate::cli::BriefArgs;
use crate::repo::Repo;
use crate::skill::{self, SkillEnvelope, Trust};

pub fn run(args: BriefArgs) -> Result<()> {
    let repo = Repo::discover()?;
    let skills = skill::load_skills(&repo)?;

    let terms: Vec<String> = args
        .query
        .iter()
        .map(|t| t.to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();

    // Signature-verified envelopes only: a briefing must never carry
    // experience that could have been edited after signing.
    let mut hits: Vec<(usize, String, SkillEnvelope)> = skills
        .into_iter()
        .filter(|(_, env)| skill::verify_envelope(env).is_ok())
        .map(|(id, env)| {
            let score = if terms.is_empty() {
                // No query: rank purely by trust and proven usage.
                1
            } else {
                skill::score_skill(&env, &terms)
            };
            (score, id, env)
        })
        .filter(|(score, _, _)| *score > 0)
        .collect();

    hits.sort_by_key(|(score, _, env)| {
        std::cmp::Reverse((*score, trust_rank(env.trust()), env.stats.uses))
    });

    if hits.is_empty() {
        println!("# Causari briefing");
        println!();
        if terms.is_empty() {
            println!("No signed experience recorded in this repository yet.");
            println!("Run `re skill distill` after working with an agent.");
        } else {
            println!(
                "No recorded experience matches {:?}. Proceed without priors.",
                args.query.join(" ")
            );
        }
        return Ok(());
    }

    let (trusted, unverified): (Vec<_>, Vec<_>) = hits
        .into_iter()
        .partition(|(_, _, env)| env.trust() != Trust::Recorded);

    println!("# Causari briefing — experience from this repository");
    println!();
    if !terms.is_empty() {
        println!("Task: {}", args.query.join(" "));
        println!();
    }
    println!(
        "_Signed and verified by Causari. This experience was accumulated \
         across previous sessions and models; treat VERIFIED entries as \
         reliable priors and UNVERIFIED entries as risk signals._"
    );

    if !trusted.is_empty() {
        println!();
        println!("## Verified experience (worked before)");
        for (_, id, env) in trusted.iter().take(args.limit) {
            print_entry(id, env);
            let _ = skill::record_use(&repo, id);
        }
    }

    if !unverified.is_empty() {
        println!();
        println!("## Unverified attempts (no success signal — treat as risk)");
        for (_, id, env) in unverified.iter().take(args.limit) {
            print_entry(id, env);
        }
    }

    println!();
    println!(
        "_Before repeating an unverified approach, check why it left no \
         success signal: `re why <file>` or `re skill show <id>`._"
    );
    Ok(())
}

/// Higher = more trusted, for descending sort.
fn trust_rank(t: Trust) -> u8 {
    match t {
        Trust::Proven => 2,
        Trust::Verified => 1,
        Trust::Recorded => 0,
    }
}

fn print_entry(id: &str, env: &SkillEnvelope) {
    let trust = env.trust();
    println!();
    println!(
        "### {} {} — {}",
        trust.badge(),
        trust.as_str(),
        env.skill.title
    );
    println!("- skill: `{}`", &id[..10.min(id.len())]);
    if let Some(agent) = &env.skill.agent {
        let model = env
            .skill
            .model
            .as_deref()
            .map(|m| format!(" ({m})"))
            .unwrap_or_default();
        println!("- learned with: {agent}{model}");
    }
    if !env.skill.files.is_empty() {
        println!("- files: {}", env.skill.files.join(", "));
    }
    println!(
        "- evidence: exit_zero={} survived={} uses={}",
        env.skill.verification.exit_zero, env.skill.verification.survived, env.stats.uses
    );
    let trigger = env.skill.trigger.trim();
    if !trigger.is_empty() {
        println!("- task it solved: {}", first_lines(trigger, 2));
    }
}

/// First `n` lines of a prompt, joined, capped for briefing compactness.
fn first_lines(s: &str, n: usize) -> String {
    let joined = s.lines().take(n).collect::<Vec<_>>().join(" ");
    let mut out: String = joined.chars().take(200).collect();
    if joined.chars().count() > 200 {
        out.push('…');
    }
    out
}
