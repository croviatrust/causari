<p align="center">
  <img src="assets/logo-readme.png" alt="Causari — intent-addressable code" width="520">
</p>

<h3 align="center">Trace intent. Debug causality.</h3>
<p align="center"><em>Intent-addressable code for AI agents.</em></p>

<p align="center">
  <a href="https://causari.dev"><strong>causari.dev</strong></a>
  &nbsp;·&nbsp;
  <a href="https://github.com/croviatrust/causari/releases">Releases</a>
  &nbsp;·&nbsp;
  <a href="https://github.com/croviatrust/causari/discussions">Discussions</a>
  &nbsp;·&nbsp;
  <a href="#mcp-server">MCP</a>
  &nbsp;·&nbsp;
  <a href="LICENSE">License (BSL 1.1)</a>
</p>

<p align="center">
  <img alt="CI" src="https://github.com/croviatrust/causari/actions/workflows/ci.yml/badge.svg?branch=main">
  <img alt="License" src="https://img.shields.io/badge/license-BSL%201.1-7c3aed">
  <img alt="Rust" src="https://img.shields.io/badge/rust-stable-orange">
  <img alt="Platform" src="https://img.shields.io/badge/platform-linux%20%7C%20macOS%20%7C%20windows-blue">
</p>

---

> *Causari* (Latin, deponent verb): *to plead a cause, to argue why.* Because
> every line of AI-generated code deserves to be defended, traced, and
> understood.

Causari records every action an AI agent takes on your codebase — not just
the bytes that changed, but the **prompt that asked**, the **model that
answered**, the **files it read**, and the **reasoning behind the change**.

**Two capture paths, one ledger.** Where the agent runtime exposes lifecycle
hooks (Claude Code), Causari plugs in natively: `re hook claude-code` wires
`UserPromptSubmit` and `PostToolUse` into `.claude/settings.json`, and every
prompt and edit is recorded **exactly** — deterministic, no heuristic, no
confidence score needed. Where hooks don't exist (Cursor, Windsurf, Cline,
Aider, custom scripts), the universal fallback (`re proxy` + `re watch`)
observes LLM traffic and the filesystem independently, then joins them by
*content* — the code that appears in your files is found inside the
completion that produced it seconds earlier. Either way, provenance becomes
a fact, not a self-report.

You can then ask questions no version control system has ever answered:

```bash
re audit                      # zero-setup: how much AI code SURVIVED in any
                              #   git repo — retroactive, works instantly
re hook  claude-code          # native capture via agent lifecycle hooks —
                              #   exact, deterministic, no heuristic needed
re proxy                      # universal fallback: local LLM proxy captures
                              #   every prompt, token and dollar
re watch                      # passive recorder + causal join: file changes get
                              #   attributed to the real prompt, model and cost
re why    src/auth.ts:42      # who/what produced this exact line?
re trace  src/auth.ts:42      # full UPSTREAM causal cone: every event that
                              #   contributed transitively, through reads/writes
re impact <event-id>          # full DOWNSTREAM cone: what flowed from this action,
                              #   transitively (causality-aware blast radius)
re lens   src/auth.ts         # render a file with per-line provenance annotations
re find   "the JWT refactor"  # search every prompt, reasoning and message
re bisect --test "npm test"   # find the agent action that broke the build
re churn                      # measure AI code survival: how much survived vs
                              #   was rewritten, per agent, with wasted spend
re report --open              # generate a shareable HTML dashboard of AI waste
re skill  distill             # turn verified events into signed, reusable skills
re skill  export <id>         # portable Ed25519 bundle for teammates
re skill  pull <team-dir>     # sync a shared folder (Dropbox, git, NFS — no server)
re skill  trust add <label>   # trust an org signing key; unknown signers rejected
re brief  "auth migration"    # portable Markdown briefing of verified experience —
                              #   inject into ANY model's context (CLAUDE.md,
                              #   AGENTS.md, .cursorrules); lessons survive models
re fork   experiment-claude   # branch into a parallel timeline
re revert <id>                # undo an action with causal preview of what else
                              #   you are implicitly undoing
```

When an agent touches 30 files and something breaks, you don't need to read
4 000 lines of chat. You ask Causari *why* and *when*.

## `re audit` — try it on any repo, right now

No setup, no ledger, no integration. `re audit` reads your existing **git
history**, detects AI-authored commits from machine-readable metadata
(`Co-Authored-By: Claude` trailers, bot author emails, aider markers), and
measures how many of those lines are **still alive at HEAD** via `git blame`:

```console
$ re audit
Causari Survival Audit
═══════════════════════════════════════════════════
  312 commits analyzed (git-only, no Causari setup required)

Verified AI-authored: 41 commits, 6 210 introduced, 4 105 survived (66.1%)
Probable AI-assisted: 12 commits, 890 introduced, 512 survived (57.5%)

By agent (verified only)
  claude-code            5 830 lines,  3 921 survived ( 67.3%)
  github-copilot           380 lines,    184 survived ( 48.4%)
```

- `re audit vercel/next.js` audits **any public repo** without touching it —
  GitHub `owner/repo` shorthand or any git URL, cloned to a temp dir and
  cleaned up afterwards. Audit the repos everyone argues about.
- `re audit --badge` writes a shields-style `causari-badge.svg`
  (`AI survival: 67.3%`) to embed in your README.
- `re audit --card` writes a shareable SVG survival card.
- `re audit --json` emits machine-readable output.
- `re audit --summary` emits Markdown — drop
  [`.github/workflows/audit.yml`](.github/workflows/audit.yml) into any repo
  and every PR gets a survival comment automatically.
- `re audit --save` appends a snapshot so you can track the trend over time.

Every number carries its evidence class: **VERIFIED** (explicit metadata)
is never mixed with **PROBABLE** (heuristic), and unknown commits never
enter the headline figures. This is the same "group 0" design rule as the
rest of Causari: git + filesystem are enough; integrations only add
precision.

## The Capture Engine — two paths, one ledger

Every provenance tool before Causari had the same fatal dependency: it only
worked if the agent volunteered its own history. Agents don't. Harnesses
don't expose reasoning. Nobody reports costs.

Causari removes the dependency with **two capture paths** that feed the same
append-only ledger:

### Primary: native hooks (`re hook claude-code`) — exact, deterministic

Where the agent runtime exposes lifecycle hooks, Causari plugs in directly.
No inference, no heuristic, no confidence score — the agent *declares* what
it did, and Causari records it:

```bash
$ re hook claude-code
causari: Claude Code hooks installed in .claude/settings.json
  UserPromptSubmit → captures every prompt
  PostToolUse (Edit|Write|MultiEdit|NotebookEdit) → records every edit

# The agent works normally. Every prompt and edit is recorded.
$ re why service.py:2
service.py:2
      return {"sha": BUILD_SHA, "uptime": uptime_seconds()}

introduced by 2d070eb8cf
  agent:     claude-code
  tool:      Write
  prompt:    Add a health-check endpoint returning build sha and uptime
```

The hook path is **deterministic by construction**: the prompt text comes
from `UserPromptSubmit`, the file path from `PostToolUse`. There is no
guesswork. Human edits that happen without a hook firing are correctly
reported as *"no recorded event introduced this line"* — zero false
attribution.

### Universal fallback: proxy + watch (`re proxy` + `re watch`) — heuristic join

For agents without hook support (Cursor, Windsurf, Cline, Aider, custom
scripts), Causari observes two independent streams and joins them by content:

```
   ┌─────────────────────────┐        ┌─────────────────────────┐
   │        re proxy         │        │        re watch         │
   │                         │        │                         │
   │  sees every prompt,     │        │  sees every byte that   │
   │  completion, token and  │        │  changes on disk        │
   │  dollar (OpenAI- and    │        │  (snapshots, diffs)     │
   │  Anthropic-compatible)  │        │                         │
   └────────────┬────────────┘        └────────────┬────────────┘
                │                                  │
                │         CONTENT-BASED JOIN       │
                └────────────────►◄────────────────┘
                 the lines inserted in your files
                 are searched inside the completions
                 captured moments before — a match is
                 a causal fingerprint, with confidence
```

A real session, end to end:

```
$ re proxy
causari: LLM capture proxy listening on http://127.0.0.1:4242
  • gpt-4o  42→18 tok  $0.0003  "Add JWT refresh logic that rotates every 24h"

$ re watch          # in another terminal
  • 0d47599550  auth.py
    ↳ intent: "Add JWT refresh logic that rotates every 24h"  gpt-4o (confidence 100%, 5/5 lines)

$ re why auth.py:3
auth.py:3
      rotated = rotate_every(session.token, hours=24)

introduced by 0d47599550
  agent:     proxy-watch
  model:     gpt-4o
  prompt:    Add JWT refresh logic that rotates every 24h
```

Point any agent at the proxy:

```bash
OPENAI_BASE_URL=http://127.0.0.1:4242/openai/v1
ANTHROPIC_BASE_URL=http://127.0.0.1:4242/anthropic
```

The causal join is a **heuristic** — it works well on clean cases but
degrades honestly on dirty ones. See [Reading the confidence
score](#reading-the-confidence-score) for real measured numbers and known
failure modes.

Everything stays on your machine: `.causari/capture/` is a local,
append-only ledger. No cloud, no telemetry, no API keys touched.

### What Causari captures — and keeping secrets out

Two streams feed the ledger, so it helps to know exactly what each one sees:

- **`re proxy`** records the *prompts, completions, token counts and costs* that
  pass through it, verbatim. If a secret is pasted into a prompt or echoed in a
  completion it is captured as-is — Causari does not yet redact prompt or
  completion text.
- **`re watch` / `re record`** snapshot your working tree. Excluded by default
  and never entering a snapshot: `.causari`, `.git`, `node_modules`, `target`,
  `dist`, `build`, `.next`, `.venv`, `__pycache__`, `.idea`, `.vscode`, and
  **`.env` / `.env.*` files** (they usually hold credentials). Everything else
  in the tree is fair game.

Practical guidance:

- Keep real secrets in `.env` / `.env.*` (excluded by default) or outside the
  repo — not hard-coded in tracked source files, which *are* snapshotted.
- The whole ledger stays local in `.causari/`, which `re init` now adds to your
  `.gitignore`, so captured prompts and reasoning are never pushed. If
  `.causari/` was committed before you upgraded, untrack it with
  `git rm -r --cached .causari && git commit -m "stop tracking .causari"`.
- Don't paste API keys, tokens or passwords into prompts while `re proxy` is
  running, and treat `.causari/capture/` as sensitive.

Configurable ignore patterns and prompt redaction are on the roadmap; today the
exclusion list above is the built-in default.

### Reading the confidence score

The causal join is a *heuristic*: it attributes a file change to a prompt by
searching the lines you inserted inside the completions captured moments before.
The **confidence score** is the fraction of inserted lines it could match back
to a captured completion (e.g. `confidence 100%, 5/5 lines`).

These numbers are **measured, not theoretical**. A reproducible test harness
(`examples/real-session/`) exercises four adversarial scenarios against a real
`re proxy` + `re watch` + mock LLM pipeline:

| Scenario | Confidence | What it means |
|---|---|---|
| **Clean** (file == completion verbatim) | **100% (5/5)** | The happy path: every line matches. |
| **Human manual edit** | no correlation | Correct silence: the edited line never appeared in any completion. No false attribution. |
| **Formatter reflow** | **50% (3/6)** | A formatter rewrote the model output before it hit disk. Confidence correctly signals degraded correspondence. |
| **Near-simultaneous prompts** | **75% (3/4)** | Two completions in the window, one file mixes lines from both. The join picks the best-overlap winner — **per-line attribution can be wrong** (the redis line was attributed to the `connect_db` prompt). |

A **high** score means the code on disk is, line for line, what the model
returned. Confidence drops when:

- **manual edits** — you hand-wrote or tweaked the code, so it never appeared in
  a completion;
- **near-simultaneous changes** — two prompts (or a prompt and a manual edit)
  touched the same file within one snapshot window, so the lines interleave;
- **post-processing** — a formatter, linter or codemod rewrote the model's
  output before it reached disk;
- **coincidental matches** — trivial lines (`}`, blank lines, common imports)
  can match unrelated completions, so they are down-weighted.

Treat a low score as *"attribution is uncertain here"*, not *"the tool is
wrong"*: run `re why <file>:<line>` or `re trace` to see the candidate events
and decide for yourself. Attribution never blocks capture — every change is
still recorded; only the *link* to a prompt is scored.

**Known failure mode (near-simultaneous prompts):** the current `correlate()`
picks the single best-matching exchange for the *whole file change*. When one
file mixes contributions from multiple prompts, individual lines can be
mis-attributed. A per-line or per-hunk join is on the roadmap. Until then,
use `re hook claude-code` (deterministic, no heuristic) where available.

### Crovia Seals — a cryptographic receipt for every completion

Causari is the **first production issuer of
[Crovia Seals](https://croviatrust.com/registry/seal/)** — the open,
IETF-drafted receipt format for AI outputs
([draft-crovia-seal-01](https://datatracker.ietf.org/doc/draft-crovia-seal/)).
One flag turns the proxy into a sealing gateway:

```
$ re proxy --seal
causari: Crovia Seal issuer active — pubkey 3fa9c2…
  • gpt-4o  42→18 tok  $0.0003  "Add JWT refresh logic"  🔏 cs_2026_Q7RM2KJ3VWXA5YBN4CDEFGH2I6
```

Every completion gets an Ed25519-signed, hash-chained, offline-verifiable
receipt in `.causari/seal/seals.jsonl`. The seal commits to SHA-256 hashes
of the exact request and response bytes — **content never leaves your
machine**. Anyone holding your public key can verify the whole chain
without a server, an account, or Causari itself:

```
$ re seal verify
✓ 128 seal(s) verified — every signature valid, chain contiguous from genesis

$ re seal issuer     # print the pubkey to share with auditors
$ re seal list       # browse issued receipts
```

The implementation is proven against the normative conformance vectors
from [croviatrust/crovia-seal](https://github.com/croviatrust/crovia-seal)
(CSC-1 canonicalization, domain-separated payloads, fail-closed
verification). When a regulator, a customer or a court asks *"which model
wrote this code, and can you prove it?"* — the answer is one file and one
public key.

## The Experience Layer — skills with earned trust

Recording the past is half the job. The other half is making sure no agent
ever pays for the same lesson twice.

`re skill distill` walks the ledger and compresses every completed task —
the prompt that triggered it, the steps that were taken, the files that
changed — into a **skill**: a unit of experience an agent can recall
*before* acting. Each skill is signed with the repository's **Ed25519 key**
at the moment of distillation; edit one byte afterwards and
`re skill verify` exposes it.

Trust is earned, never claimed:

```
●  recorded   distilled from the ledger — no success signal yet
◆  verified   evidence attached: exit code 0, or the work is still
              alive at the tip of the timeline (it survived)
★  proven     verified AND recalled 3+ times by agents doing new work
```

```
$ re skill distill
distill: 128 event(s) scanned, 7 new skill(s), 12 already distilled
  ◆ verified 2ce0c7bbda  add retry with exponential backoff

$ re skill verify
  ok 2ce0c7bbda  add retry with exponential backoff
verify: 7 skill(s), every signature valid
```

The loop closes through MCP: when an agent calls `causari_recall`, **signed
skills are returned first, ranked by trust** (proven ×4, verified ×2), and
every recall bumps the skill's use counter — which is exactly how a
verified skill earns the ★. Agents get measurably cheaper over time, and
`re churn` shows you the savings in dollars.

### Team skill mesh — no server, no accounts

One engineer's verified fix becomes every agent's instinct — without a
central SaaS:

```bash
re skill export 2ce0c7bbda --output jwt-fix.json   # portable bundle
re skill trust pubkey                             # share your Ed25519 key
re skill trust add platform <their-pubkey>        # trust a teammate/org key
re skill import jwt-fix.json                      # verify signature + accept
re skill pull ~/Dropbox/causari-skills/           # sync a whole team folder
```

Skills signed by unknown keys are **rejected**, not imported. The mesh is
cryptographic: Dropbox, git, NFS, S3 — any folder works. Causari verifies
Ed25519 on every file; tampered bundles fail closed.

Like everything in Causari, skills are local files (`.causari/skills/`),
self-contained and portable. The signature means a skill can be shared and
*verified by anyone* — across repos, teams, and orgs, with no central server.

## Causari Proof — verifiable AI provenance, trustless

Every repo can mint a **signed proof of its AI provenance** — how many agent
actions, which agents and models, how much *verified* experience — bound to the
exact ledger by a content digest and signed with the repo's Ed25519 key.

```bash
re proof generate            # → causari-proof.json + causari-proof.svg badge
re proof verify              # checks the signature offline — no server, no account
re proof verify --against-repo   # …and confirms it still matches the live ledger
```

Anyone — a reviewer, an auditor, a stranger reading your PR — can run
`re proof verify` and confirm the proof was **not altered after signing**. No
Causari account, no network call, no trust in us. Tamper with a single number
and verification fails closed.

Drop the badge in your README and every visitor sees it:

```markdown
[![AI provenance — verified by Causari](causari-proof.svg)](https://causari.dev/verify)
```

It is agent-agnostic by construction: the proof aggregates the *ledger*, so it
covers every agent Causari captured — Claude Code, Cursor, Cline, Windsurf, a
raw `re proxy` — not just one runtime.

**Free forever:** generating and verifying proofs offline. **Commercial (Trust
Plane):** the hosted public verification page on `causari.dev`, the org-wide
proof registry, RFC 3161 timestamp anchoring, and audit-grade compliance
exports.

## What makes it different

Existing tools either track text (git), track sessions (IDE checkpoints), or
track conversations (LangSmith, Helicone). **None of them connect a line of
code to the intent that produced it** — and none of them can do it without
the agent's cooperation. Causari does both:

| You ask… | Causari answers… |
|---|---|
| **`re hook claude-code`** | **Native, deterministic capture.** Wires into agent lifecycle hooks — exact prompt and tool attribution, no heuristic, zero false positives on human edits. The recommended primary path. |
| **`re proxy` + `re watch`** | **Universal fallback.** Prompts, models, tokens and dollars joined to file changes by content correlation — works with any agent, no cooperation required. Heuristic-based; see [confidence score](#reading-the-confidence-score) for measured limits. |
| `re why src/auth.ts:42` | The prompt, model, agent, tool, and reasoning that wrote that line. |
| **`re trace src/auth.ts:42`** | **Upstream causal cone.** Every prior event that contributed, transitively, through the files it read or wrote. The intellectual ancestry of a piece of code. |
| **`re impact <event>`** | **Downstream causal cone.** Every later event that depended, transitively, on what this one produced. The blast radius of an action. |
| **`re lens src/auth.ts`** | The file rendered with **per-line provenance annotations**: each line painted with the event id that introduced it. |
| `re find "the JWT refactor"` | Signed **skills first**, then every event — prompt, message, reasoning — ranked by trust and relevance. |
| `re bisect --test "<cmd>"` | The first agent action whose output fails your tests. |
| **`re churn`** | **AI Waste Score.** How much AI-written code survived vs was rewritten, per agent. With cost data: dollars spent on code that did not survive. |
| **`re report --open`** | A **self-contained HTML dashboard** you can paste into Slack, PRs, or board decks — zero external assets, zero cloud calls. |
| **`re skill distill`** | **Signed experience.** Verified past work compressed into Ed25519-signed skills, recalled by agents (trust-ranked) before they act — the same mistake is never paid twice. |
| **`re skill export` / `pull`** | **Team skill mesh.** Portable bundles + trusted org keys; sync any shared folder. Unknown signers and tampered files rejected. |
| **`re proof generate` / `verify`** | **Trustless AI-provenance certificate.** A signed, content-bound proof + embeddable badge that *anyone* can verify offline — no server, no account. Tampering fails closed. |
| `re fork claude-attempt` | A new timeline you can extend without touching the original. |
| **`re watch --session bot1`** | **Concurrent multi-agent recording.** One session per agent, lock-serialized commits, shared ancestry — no agent can orphan another's events. |
| `re sessions` / `re switch <name>` | The fleet overview: every session tip with agent and last activity; jump between timelines. |
| `re log --all` | The full event DAG across every session, with tip and fork-point markers. |
| `re diff a..b` | The exact file delta between two agent actions. |
| `re revert <id>` | Workspace snapped back to the pre-state of that action, **with a causal preview** of every downstream event you are implicitly undoing. |

### The bidirectional causal graph

Most version control is one-dimensional: a chain of commits. Causari is two-dimensional:

```
                 PAST                            FUTURE
   ┌──────────────────────────┐  ┌──────────────────────────┐
   │                          │  │                          │
   │   re trace foo.rs:42     │  │   re impact <event>      │
   │                          │  │                          │
   │   ← prompts & events     │  │   events & prompts →     │
   │   that produced this     │  │   that flowed from this  │
   │                          │  │                          │
   └──────────────┬───────────┘  └─────────────┬────────────┘
                  │                            │
                  │       a single event       │
                  └────────────►●◄─────────────┘
```

This unlocks a question nothing else can answer:

> *"If I revert this action, what else am I implicitly undoing?"*

`re revert` answers it before touching a single byte.

### Why `re trace` matters

Git blame names one author. `re why` names one event. **`re trace`** reconstructs
the *intellectual ancestry* of a piece of code:

```
calc.js:2
  export function sum(a, b) { return a - b; }

trace: 3 causal contributors found

● 0b8424ee83  align calc.js with updated spec
   agent: gpt-4o
   prompt: the spec was updated, make calc.js match
   because: wrote calc.js:2
  └─ 45230e9cda  update spec to redefine sum
     agent: gpt-4o
     prompt: the team decided sum should compute a-b, update the spec
     because: wrote spec.md which event 0b8424ee83 read
  └─ 55a6dd9392  implement calc per spec
     agent: claude-3.5
     prompt: implement sum() following the spec in spec.md
     because: wrote calc.js which event 0b8424ee83 read
```

The buggy line is not the root cause — the *prompt that asked the agent to
redefine the spec* is. Causari surfaces it. **You can debug prompts, not just
code.**

## How it works

Every event is a content-addressable object (BLAKE3) containing:

- `pre_snapshot` and `post_snapshot` — the workspace tree before and after
- `agent`, `model`, `tool`
- `prompt` — the user task that triggered the action
- `reasoning` — the agent's chain-of-thought when exposed
- `reads`, `writes`, `tokens_in`, `tokens_out`, `cost_usd`
- `parent` — the previous event in the timeline

Snapshots are incremental (only changed files create new blobs, just like
git's object store), so the storage cost is bounded by the *delta*, not the
absolute size of the workspace.

## Causari Log

```
[09:23:01]  re init
              → .causari/ repository initialized

[09:24:33]  re record -m "Add JWT refresh logic"
              → 12 lines in src/auth.ts
              → agent: claude-3.5-sonnet
              → prompt: "Add JWT refresh logic that rotates every 24h"

[09:25:12]  re guard
              ⚠ critical without test: src/auth.ts
              ⚠ 1 alert(s), 0 warning(s)

[09:25:45]  re why src/auth.ts:5
              → introduced by a3f7b2c9
              → prompt: "Add JWT refresh logic that rotates every 24h"
              → reasoning: The spec calls for refresh tokens...
              → 3 seconds

[09:26:18]  re trace src/auth.ts:5
              → a3f7b2c9 "Add JWT refresh"
                └─ e112706e "Update auth spec"
                  └─ c13aa663 "Initial scaffold"

[09:27:03]  re guard --badge
              ✓ .causari/guard-badge.svg generated

[09:27:44]  re impact a3f7b2c9
              → downstream: 2 events depend on this
              → c4d1e8f2 "Deploy to staging"
              → d5e2a1b3 "Fix OAuth scope"

[09:28:19]  re churn
              causari churn: code survival across 1,284 events
                AGENT          INTRO  SURVIVED  WASTE    WASTED $
                claude-3.5     8,210    6,012   26.8%    $164.10
                gpt-4o         3,400    1,510   55.6%    $116.90
                cursor         1,120      980   12.5%      $5.50

              AI survival 66.8% · AI Waste Score 33.2%
              $286.50 of $866.90 spent on code that did not survive

[09:29:02]  re report --open
              ✓ report written to causari-report.html
              → opening in browser

[09:29:33]  re revert a3f7b2c9
              ⚠ preview: 2 downstream events will lose context
              → confirm with --yes to proceed
```

Next on the roadmap:

- ~~**Verified skills (experience layer)**: events whose verification passed
  get promoted into signed, reusable skills the agent can recall *before*
  acting~~ ✓ shipped: `re skill distill/list/show/verify`, Ed25519
  signatures, trust ladder (● recorded → ◆ verified → ★ proven),
  trust-ranked `causari_recall` via MCP
- ~~**Multi-agent DAG timelines**: concurrent agents, true branching history~~
  ✓ shipped: `--session`, `re sessions`, `re switch`, `re log --all`
- ~~**Team skill registry**: share signed skills across an organization —
  one engineer's verified fix becomes every agent's instinct~~ ✓ shipped:
  `re skill export/import/pull`, `re skill trust` — Ed25519 mesh, no server
- ~~**Verifiable provenance certificate**: a signed, content-bound proof of a
  repo's AI provenance that anyone can verify offline, with an embeddable
  badge~~ ✓ shipped: `re proof generate/verify`, Ed25519, self-contained SVG,
  tamper-evident (fails closed)
- Cryptographic timestamps (RFC 3161) + Ed25519-signed events for
  audit-grade timelines (EU AI Act, SOC2 for agentic development) — *the paid
  Trust Plane on top of `re proof`*
- **Agent Provenance Protocol**: an open spec for the signed,
  content-addressed event format, so any tool can produce or verify it
- TUI à la `lazygit` for visual exploration
- Cross-event semantic search over prompts and diffs (embeddings)
- Counterfactual `re replay --with <model>` (re-execute past events under different models)

## Plugging Causari into your agent (MCP)

Causari ships its own MCP server. Any agent runtime that speaks MCP
(Claude Desktop, Claude Code, Cursor, Cline, Windsurf, …) can register
Causari and get three new tools for free:

| Tool | What the agent uses it for |
|---|---|
| `causari_record` | Record one of its own actions into the ledger after each tool call. |
| `causari_recall` | Find past similar events *before* acting, to avoid repeating mistakes. |
| `causari_why`    | Inspect the provenance of a line before modifying code it didn't write. |

**Trust model — what each tool can touch.** The server runs locally over stdio
(newline-delimited JSON-RPC 2.0), makes **no network calls**, and only ever
reads or writes inside your project and its `.causari/` ledger. Nothing is sent
to Causari or any third party.

| Tool | Reads | Writes |
|---|---|---|
| `causari_record` | your working tree (to snapshot it) + the metadata you pass in | appends one immutable event + snapshot to `.causari/` — **never edits your source files** |
| `causari_recall` | `.causari/` ledger and signed skills | only bumps a skill's use-counter in `.causari/` (how trust is earned) — never your source |
| `causari_why`    | `.causari/` ledger + the single file/line you name | nothing |

Get the JSON snippet to paste into your agent's config:

```bash
re mcp --install
```

Then in any conversation the agent can call those tools by name. Causari
silently builds a complete, queryable, causally-linked history of the session.

## CI / GitHub Action

One step in any repo and every pull request gets an AI code-survival
comment — zero cloud, zero configuration, no Causari setup required:

```yaml
# .github/workflows/causari.yml
name: Causari Audit
on:
  pull_request:

permissions:
  contents: read
  pull-requests: write

jobs:
  audit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0        # full history: audit reads every commit
      - uses: croviatrust/causari@main
```

It downloads the prebuilt `re` binary (seconds, no Rust toolchain), runs
`re audit --summary`, writes the result to the job summary, and posts a
sticky Markdown comment on the PR with verified survival numbers per agent.

For risky-pattern alerts, add `re guard --summary` as an extra step —
see [`.github/workflows/guard.yml`](.github/workflows/guard.yml).

Keep the badge green on `main`:

```bash
re guard --badge   # .causari/guard-badge.svg
```

## Quickstart

### Install (one line)

Linux & macOS:

```bash
curl -sSf https://causari.dev/install.sh | sh
```

Windows (PowerShell):

```powershell
iwr -useb https://causari.dev/install.ps1 | iex
```

Installs a ~800 KB pre-built binary into `~/.local/bin` (or
`%LOCALAPPDATA%\Programs\causari` on Windows). **The installer verifies the
binary's SHA-256 against the `SHA256SUMS.txt` published with each
[release](https://github.com/croviatrust/causari/releases) and refuses to
install on a mismatch** (set `CAUSARI_SKIP_VERIFY=1` to bypass — not
recommended).

Prefer to verify by hand before running anything?

```bash
VERSION=v0.1.0                        # the release you want
TARGET=x86_64-unknown-linux-gnu       # your platform triple
base="https://github.com/croviatrust/causari/releases/download/$VERSION"
curl -sSfLO "$base/re-$VERSION-$TARGET.tar.gz"
curl -sSfLO "$base/SHA256SUMS.txt"
sha256sum --ignore-missing -c SHA256SUMS.txt   # expect: OK
tar -xzf "re-$VERSION-$TARGET.tar.gz" && install -m755 re ~/.local/bin/re
```

Or build from source and skip pre-built binaries entirely:

```bash
cargo install --git https://github.com/croviatrust/causari
# or, with a local clone:
cargo build --release
./target/release/re --help
```

On first run, `re init` creates the `.causari/` ledger and adds it to your
`.gitignore` automatically, so the prompts and reasoning it captures are never
committed.

Scripted demos live in `scripts/` and `examples/`:

- **`examples/real-session/`** — **adversarial test harness with real measured
  numbers.** Exercises the causal join on clean, human-edited, formatter-reflowed,
  and near-simultaneous scenarios; compares hook vs proxy paths. See
  [`RESULTS.md`](examples/real-session/RESULTS.md) for the findings.
- **`demo-capture.ps1`** — the capture engine end to end: mock LLM upstream,
  `re proxy`, `re watch`, content-based causal join (`mock-llm.py` included)
- **`demo.sh`** / **`demo.ps1`** — full happy-path with `re why` and `re bisect`
- **`demo-trace.sh`** / **`demo-trace.ps1`** — upstream causal cone (`re trace`)
- **`demo-bidir.sh`** / **`demo-bidir.ps1`** — bidirectional causality
  (`re impact`, `re lens`, causality-aware `re revert`)
- **`demo-mcp.sh`** / **`demo-mcp.ps1`** — MCP server end-to-end via JSON-RPC

Causari runs natively on **Linux, macOS, and Windows** — the binary is a
single ~2 MB executable with no runtime dependencies.

## License

Causari is released under the **Business Source License 1.1** (see
`LICENSE`). In plain English:

- **Free for you, forever**, in all of these cases:
  - personal use, including paid client work;
  - any organization's own internal development, in production and CI/CD,
    at any scale;
  - use by AI agents acting on infrastructure you control (laptop, server,
    CI runner) — this is the default way Causari is meant to be used;
  - academic research, teaching, non-commercial open-source projects;
  - redistribution of unmodified binaries via cargo / brew / apt / choco /
    nix / winget.
- **Not free** if you want to resell Causari itself to third parties as a
  hosted or managed service whose primary value is version control,
  causality or provenance tracking for AI agents. For that, talk to us.
- **Becomes Apache 2.0 automatically** four years after each version is
  published. The change is mechanical: nothing the project maintainers can
  prevent or accelerate.

**The experience layer (skills) is and will remain free** in the `re`
binary — distillation, Ed25519 signing, verification and recall all run
locally and cost nothing. What will be commercial is the **Trust Plane**
built on top of it: organization-wide signed skill registries, RFC 3161
timestamping, fleet dashboards and audit-grade compliance exports. The
free tool creates the experience; the paid plane lets a company trust it
at scale.

Why BSL? Causari is meant to be widely used and modified, but we are not
interested in subsidizing a future closed-source clone built by a
better-distributed competitor. This is the same model that lets Sentry,
HashiCorp and CockroachDB stay genuinely useful and contributable while
sustaining the people who build them.

Contributing? See `CLA.md`.

---

Causari is built by [Croviatrust](https://croviatrust.com) — the team behind
**Crovia**, the cryptographically signed, Bitcoin-anchored public ledger of
AI training-data transparency. Same DNA, different layer: Crovia proves what
models learned; **Causari proves what agents did.**
