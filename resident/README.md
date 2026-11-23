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
- Unfinished research can carry observations tied to an actually read file and
  commit SHA, plus a precise next question, into the next generation. Observations
  remain explicitly unverified. Continuation is bounded to three attempts before
  focus selection explores another direction.
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
The final two inference turns are reserved for a decision or a research handoff,
so inspection cannot silently consume the entire cycle without a conclusion.

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

## Research fleet and capability library

The resident now runs three separate model contexts on the same runner:

1. A scout chooses a concrete research question, public repository sources and
   official documentation. It defines the mission of a generated specialist.
2. The specialist inspects and edits the actual project using the existing bounded
   actions, with external material clearly marked as untrusted evidence.
3. An independent critic can veto the experiment. Acceptance means worth testing,
   not proof that it works. The isolated execution job is still the evidence gate.

The shared budget is eight model calls: one scout, up to six specialist calls,
then one critic. These agents run sequentially; this version does not claim
parallel speedup or independently hosted child repositories. Specialist identities
are derived from their missions. The retained lineage contains at most six
specialists plus the scout and critic, and records attempts and source-case
verification outcomes.

The scout can search public GitHub repositories and read files at pinned commit
SHAs. A source record preserves the repository, path, revision, content hash,
license metadata and retrieval time. Documentation retrieval is limited to the
hosts coded in `research.mjs`; private repositories, arbitrary destinations,
account APIs and cross-host token redirects are refused. External material is
never installed or executed. Failed searches are recorded; they do not become
fabricated evidence. Reading a source or its license metadata does not grant
permission to copy it.

A code experiment becomes a capability capsule only when the critic accepts it
and independent verification proves the before/after change and original suite.
Capsules contain the actual repair code, a runnable regression, contract,
source provenance and specialist lineage. The memory branch stores detailed
capsules under `capabilities/`, with at most 24 compact entries in its index.
Each project can retrieve matching recipes from its sibling. Retrieval is not
proof of successful transfer: adaptation to a new case must be verified again.
These are tested repair recipes, not model weight updates or unrestricted tools.

For independently verified experiments only, the writer also stores an English
communication draft under `outreach/`. It distinguishes the hypothesis, intended
benefit and observed test outcomes. No social account, email or discussion is
contacted, and no claims about customers, revenue or broad transfer are generated
as observed results. The draft is material for review and subsequent publication.

`node --test resident/*.test.mjs` includes fixture-based complete research,
proposal, critique, execution, capsule promotion and cross-repository retrieval.
Model responses in these fixtures are test doubles. `docker-proof.mjs` separately
uses real containers for the source regression and promotion gate. Neither is a
claim that the live model has autonomously acquired or transferred a useful skill.
