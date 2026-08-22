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
    let terms: Vec<String> = args
        .query
        .iter()
        .map(|t| t.to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();

    match render(&repo, &terms, args.limit, true)? {
        Some(md) => print!("{md}"),
        None => {
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
        }
    }
    Ok(())
}

/// Render the trust-ranked experience briefing as Markdown.
///
/// Returns `Ok(None)` when nothing matches — callers that inject context
/// automatically (e.g. the SessionStart hook) must stay silent in that case.
/// `bump` controls whether briefed skills earn a recall (★ proven ladder);
/// automatic injection passes `false` so trust is only earned by explicit use.
pub fn render(repo: &Repo, terms: &[String], limit: usize, bump: bool) -> Result<Option<String>> {
    use std::fmt::Write as _;

    let skills = skill::load_skills(repo)?;

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
                skill::score_skill(&env, terms)
            };
            (score, id, env)
        })
        .filter(|(score, _, _)| *score > 0)
        .collect();

    hits.sort_by_key(|(score, _, env)| {
        std::cmp::Reverse((*score, trust_rank(env.trust()), env.stats.uses))
    });

    if hits.is_empty() {
        return Ok(None);
    }

    let (failures, rest): (Vec<_>, Vec<_>) = hits
        .into_iter()
        .partition(|(_, _, env)| env.skill.verification.failed);
    let (trusted, unverified): (Vec<_>, Vec<_>) = rest
        .into_iter()
        .partition(|(_, _, env)| env.trust() != Trust::Recorded);

    let mut out = String::new();
    out.push_str("# Causari briefing — experience from this repository\n\n");
    if !terms.is_empty() {
        let _ = writeln!(out, "Task: {}\n", terms.join(" "));
    }
    out.push_str(
        "_Signed and verified by Causari. This experience was accumulated \
         across previous sessions and models; treat VERIFIED entries as \
         reliable priors and UNVERIFIED entries as risk signals._\n",
    );

    if !trusted.is_empty() {
        out.push_str("\n## Verified experience (worked before)\n");
        for (_, id, env) in trusted.iter().take(limit) {
            push_entry(&mut out, id, env);
            if bump {
                let _ = skill::record_use(repo, id);
            }
        }
    }

    if !unverified.is_empty() {
        out.push_str("\n## Unverified attempts (no success signal — treat as risk)\n");
        for (_, id, env) in unverified.iter().take(limit) {
            push_entry(&mut out, id, env);
        }
    }

    if !failures.is_empty() {
        out.push_str(
            "\n## Known failures (recorded non-zero exit — do NOT repeat this approach)\n",
        );
        for (_, id, env) in failures.iter().take(limit) {
            push_entry(&mut out, id, env);
        }
    }

    out.push_str(
        "\n_Before repeating an unverified approach, check why it left no \
         success signal: `re why <file>` or `re skill show <id>`._\n",
    );
    Ok(Some(out))
}

/// Higher = more trusted, for descending sort.
fn trust_rank(t: Trust) -> u8 {
    match t {
        Trust::Proven => 2,
        Trust::Verified => 1,
        Trust::Recorded => 0,
    }
}

fn push_entry(out: &mut String, id: &str, env: &SkillEnvelope) {
    use std::fmt::Write as _;

    let trust = env.trust();
    let _ = writeln!(
        out,
        "\n### {} {} — {}",
        trust.badge(),
        trust.as_str(),
        env.skill.title
    );
    let _ = writeln!(out, "- skill: `{}`", &id[..10.min(id.len())]);
    if let Some(agent) = &env.skill.agent {
        let model = env
            .skill
            .model
            .as_deref()
            .map(|m| format!(" ({m})"))
            .unwrap_or_default();
        let _ = writeln!(out, "- learned with: {agent}{model}");
    }
    if !env.skill.files.is_empty() {
        let _ = writeln!(out, "- files: {}", env.skill.files.join(", "));
    }
    let _ = writeln!(
        out,
        "- evidence: exit_zero={} survived={}{} uses={}",
        env.skill.verification.exit_zero,
        env.skill.verification.survived,
        if env.skill.verification.failed {
            " FAILED"
        } else {
            ""
        },
        env.stats.uses
    );
    let trigger = env.skill.trigger.trim();
    if !trigger.is_empty() {
        let _ = writeln!(out, "- task it solved: {}", first_lines(trigger, 2));
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
