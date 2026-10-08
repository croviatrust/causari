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

    match render(&repo, &terms, args.limit)? {
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
/// A briefing does not write the legacy recall count and does not change trust.
pub fn render(repo: &Repo, terms: &[String], limit: usize) -> Result<Option<String>> {
    use std::fmt::Write as _;

    // Signature-verified AND currently-trusted signers only: a briefing must
    // never carry experience that could have been edited after signing, nor
    // experience from a key that has since been revoked.
    let skills = skill::load_admissible_skills(repo)?;
    let mut hits: Vec<(usize, String, SkillEnvelope)> = skills
        .into_iter()
        .map(|(id, env)| {
            let score = if terms.is_empty() {
                // No query: rank by the declared signal only.
                1
            } else {
                skill::score_skill(&env, terms)
            };
            (score, id, env)
        })
        .filter(|(score, _, _)| *score > 0)
        .collect();

    hits.sort_by_key(|(score, _, env)| std::cmp::Reverse((*score, trust_rank(env.trust()))));

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
    out.push_str(&briefing_limit());

    if !trusted.is_empty() {
        out.push_str("\n## Declared outcome signal\n");
        for (_, id, env) in trusted.iter().take(limit) {
            push_entry(&mut out, id, env);
        }
    }

    if !unverified.is_empty() {
        out.push_str("\n## No declared outcome signal\n");
        for (_, id, env) in unverified.iter().take(limit) {
            push_entry(&mut out, id, env);
        }
    }

    if !failures.is_empty() {
        out.push_str(
            "\n## Known failures (caller-supplied non-zero exit, no exit 0 — do not repeat this approach)\n",
        );
        for (_, id, env) in failures.iter().take(limit) {
            push_entry(&mut out, id, env);
        }
    }

    out.push_str(
        "\n_Before repeating an approach with no declared outcome signal, \
         read it: `re skill show <id>`. A briefing is not `re audit`._\n",
    );
    Ok(Some(out))
}

/// The human limit on a briefing. The machine words stay `verified` and `recorded`.
fn briefing_limit() -> String {
    format!(
        "_Ed25519 signs each skill file so a later edit is detectable. That check is not an outcome \
         and not the audit field `verified`. \
         {} \
         `recorded` means that declared signal is absent. \
         `failed` is a caller-supplied non-zero exit with no exit 0. \
         `proven` is not awarded. A legacy recall count may remain on the file; it is not an execution \
         and does not change trust. \
         None of this proves the approach was correct or that a model typed the code._\n",
        skill::VERIFIED_GLOSS
    )
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
        "- declared: exit_zero={} survived={} failed={}",
        env.skill.verification.exit_zero,
        env.skill.verification.survived,
        env.skill.verification.failed,
    );
    let _ = writeln!(out, "- observed success: none recorded");
    let _ = writeln!(
        out,
        "- legacy recalls: {} (not executions; ignored for trust)",
        env.stats.uses
    );
    let trigger = env.skill.trigger.trim();
    if !trigger.is_empty() {
        let _ = writeln!(out, "- trigger: {}", first_lines(trigger, 2));
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

#[cfg(test)]
mod wording_tests {
    use super::*;

    #[test]
    fn briefing_does_not_promote_the_recall_ladder_to_a_proof() {
        let text = briefing_limit();
        assert!(text.contains("`proven` is not awarded"));
        assert!(text.contains("not an execution"));
        assert!(text.contains("not the audit field `verified`"));
        assert!(text.contains("None of this proves the approach was correct"));
        assert!(text.contains("declared signal frozen at distill"));
        assert!(text.contains("does not certify the content"));
        assert!(text.contains("not measured reliability"));
        assert!(!text.contains("at least 3"));
    }

    #[test]
    fn repeated_briefs_do_not_promote_or_rewrite_the_counter() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repo::init(tmp.path()).unwrap();
        let key = skill::load_or_create_signing_key(&repo).unwrap();
        let core = skill::SkillCore {
            schema: skill::SKILL_SCHEMA.into(),
            title: "paint the note blue".into(),
            trigger: "paint the note blue".into(),
            steps: vec![],
            agent: Some("fixture".into()),
            model: None,
            source_events: vec!["e1".into()],
            files: vec!["note.txt".into()],
            verification: skill::Verification {
                exit_zero: true,
                survived: true,
                failed: false,
            },
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let mut env = skill::sign_skill(core, &key).unwrap();
        env.stats.uses = 7;
        let id = skill::skill_id(&env.skill).unwrap();
        skill::save_skill(&repo, &id, &env).unwrap();

        let first = render(&repo, &["paint".into()], 5).unwrap().expect("match");
        let second = render(&repo, &["paint".into()], 5).unwrap().expect("match");
        assert_eq!(
            first, second,
            "a briefing must not change what the next one reads"
        );
        assert!(first.contains("◆ verified"));
        assert!(!first.contains("★ proven"));
        assert!(first.contains("legacy recalls: 7"));
        assert!(first.contains("observed success: none recorded"));
        let (_, after) = skill::find_skill(&repo, &id).unwrap();
        assert_eq!(after.stats.uses, 7);
        skill::verify_envelope(&after).unwrap();
        assert_eq!(after.trust(), Trust::Verified);
    }
}
