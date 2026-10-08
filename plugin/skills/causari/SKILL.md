---
name: causari
description: Use when the user asks why a line or file exists, who or what wrote it, which prompt produced a change, whether an AI edit survived, or how many lines from AI-tagged commits are still at HEAD. Also use before repeating a task this repository has already tried. Report the evidence class; do not treat declared as proven or untagged as human.
---

# Causari: the causes behind code

Causari keeps a ledger of what AI agents did in this repository: for every
edit, the prompt, the model, the files and a before/after snapshot. Git says
which commit; Causari says which prompt. Everything is local, in `.causari/`.

## When to reach for it

- "Why is this line here?" / "who wrote this?" → `causari_why` (MCP) or
  `re why <file>:<line>`. Report the evidence class the tool returns
  (`declared`, `correlated`, `observed`); do not present a correlation as a
  fact.
- "Everything that led to this line, transitively?" → `re trace <file>:<line>`;
  "how did this file change over the session?" → `re log` then `re diff <id>`.
- "Which of my edits are still there?" / "how much AI code survived?" →
  `re churn --json` for the ledger, `re audit --json` for git history of any
  repository (a count, not a grade; say so).
- Before starting a task the repository may have tried before → `causari_recall`
  with a short description; it returns past events and their outcome.
- After a change you made outside the hooked tools (shell scripts, generated
  files) → `causari_record` with the prompt and the files, so the ledger stays
  complete.

## What not to claim

- Causari records what happened; it does not judge quality. Never call a
  survival number "healthy" or a repository "high churn".
- An `observed` event has no known cause. Say "cause unknown".
- `proven` is not awarded. `verified` on a skill is a declared signal frozen
  at distill (a caller-supplied exit code 0, or the declared write paths
  still exist). It is not an observed success, and the Ed25519 signature
  does not certify the content. A 2× rank weight is that declared signal,
  not measured reliability. A legacy recall count is not an execution. That
  rank is not a cryptographic proof, and it is not the audit field
  `verified` (metadata matched).
- If `re` is not installed the hooks do nothing. Suggest
  `curl -fsSL https://causari.dev/install.sh | sh` (or
  `brew install croviatrust/tap/causari`) and `re init` in the project.

## Commands worth knowing

```
re init                      start the ledger in this repository
re log                       recent events
re show <id> --json          prompt, model, tokens, cost, evidence
re why <file>:<line>         the event and prompt behind a line
re diff <id>                 what one event changed
re revert <id> --dry-run     what going back before an event would touch
```
