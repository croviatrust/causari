# Crovia resident

A GitHub-resident engineering experiment for AgentCrash and Causari, authored by Crovia.

Each hourly wake reads the real repository, its sibling, recent commits, issues,
past experiments and review outcomes. A language model chooses what to inspect
and proposes a bounded experiment. The model has read and search actions, not a
shell. A separate job runs the experiment in Docker without network access or
credentials. A final job records the generation on `crovia-memory` and saves
verified changes on `crovia/resident-<id>` branches. It never runs candidate code
with a write token and never merges changes automatically.

The process survives the runner: source and strategy live on the default branch;
observations, failures and proposals live on `crovia-memory`. Runners are
temporary, not a permanently running process. GitHub schedules can be delayed.

## What evolves

- Memory of attempts, failed hypotheses and review outcomes persists between runs.
- Focus selection explores untried directions and uses observed review outcomes.
- The model can propose a new `resident/strategy.json`, subject to a fixed schema
  and review. A schema-valid strategy is explicitly **not** a proven improvement.
- The model reads both projects, but writes only to the repository hosting it.
  Install the same files in each repository; no cross-repository write token is
  needed for branches and memory.

This is not weight training, demonstrated recursive intelligence improvement,
proof of independent invention, or a promise of business growth. The quality of
the local bootstrap model must be evaluated from actual executions. The first
version explores source code and regressions; it does not browse the web, contact
people, publish marketing, or discover unlimited new tools.

## Code experiments

Every source change requires a new regression test. The original suite must pass,
the regression must fail on the original source, and both the regression and
original suite must pass on the candidate. Failed or inconclusive attempts are
recorded rather than published as successful improvements. Existing test files,
workflow permissions, manifests and controller code cannot be edited by a proposal.

The verifier copies the committed baseline. Candidate code runs in a read-only
container with a writable temporary directory, no network, no host token, no
Docker socket, capped CPU, memory, processes and time. Rust dependencies are
fetched from the original lockfile before offline builds. The original test files
cannot be changed by the proposal. Container isolation and tests are useful checks,
not a complete security boundary or proof of correctness. All candidate PRs are drafts.

## Start and identity

After merging the workflow into the default branch, use **Actions → Crovia
resident → Run workflow**, or wait for the next hourly wake. The implementation
branch has been used for bootstrap testing. Run summaries and the memory
branch show whether it actually ran and what happened.

By default, Ollama runs `qwen3.5:4b` on the GitHub runner CPU. It needs no
external inference key. Its weights are cached; full job artifacts expire after
seven days. The CPU bootstrap model trades capability for resource cost. Do not
assume it can match a frontier coding model. The model download, image pull and
CPU inference may fail or exceed the time budget; these are execution failures,
not evidence of autonomous progress. Cache eviction is possible.

Git commits use author **Crovia <info@croviatrust.com>**. To have PRs opened by
**croviatrust**, configure an Actions secret `CROVIA_GITHUB_TOKEN` belonging to that
account with Contents and Pull requests write permissions for the two repositories.
Without it, the resident saves candidate branches and comparison links, and does
not substitute a bot account for Crovia. No token belongs in repository files.

Optional repository configuration:

| Setting | Meaning |
| --- | --- |
| Variable `CROVIA_RESIDENT_PAUSED=true` | Stop new cycles |
| Variable `RESIDENT_MODEL` | Ollama model tag, or remote model identifier |
| Variable `RESIDENT_API_URL` | HTTPS OpenAI-compatible chat-completions endpoint; skips local Ollama |
| Secret `RESIDENT_API_KEY` | Credential for that endpoint, if needed |
| Secret `CROVIA_GITHUB_TOKEN` | Open PRs as croviatrust |

External model inference may cost money; the default does not call a paid API.
Public standard GitHub runners have free compute under GitHub's current terms,
but storage, concurrency, time and acceptable-use limits still apply. This system
does not create extra repositories or chain jobs to evade those limits. Source
code and issue excerpts are sent to the configured model provider if a remote
endpoint is selected.

At three outstanding resident PRs or unsubmitted candidate branches, proposals
pause for review. Review or discard the candidates before continuing. Attention
metrics are recorded as attention, never as customers, recurring use or revenue.

## Local checks

```sh
node --test resident/core.test.mjs
```

These unit tests include a real before/after source regression in disposable local
repositories, plus test doubles for model output and container execution. They
do not claim an actual model-generated improvement or a live Docker run.
The separate `resident controller checks` workflow additionally runs
`node resident/docker-proof.mjs` against real isolated containers and proves both
acceptance of a correction and rejection of an existing-behavior regression.

Controller files are deliberately outside its own edit capability in this first
version. Proposing broader changes to its runtime requires a separately reviewed
expansion of the experiment format, preserving independent evaluation.
